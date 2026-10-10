//! Speech model download and lifecycle. The model is fetched on first run
//! rather than bundled, keeping the installer small.

use futures_util::StreamExt;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams};
use transcribe_rs::onnx::Quantization;

const MODEL_NAME: &str = "parakeet-tdt-0.6b-v3-int8";
const BASE_URL: &str = "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main";
const FILES: &[&str] = &["encoder-model.int8.onnx", "decoder_joint-model.int8.onnx", "nemo128.onnx", "vocab.txt"];
const COMPLETE_MARKER: &str = ".complete";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ModelStatus {
    NotDownloaded,
    Downloading { downloaded: u64, total: u64 },
    Loading,
    Ready,
    Error { message: String },
}

pub struct Model {
    dir: PathBuf,
    status: Mutex<ModelStatus>,
    engine: Mutex<Option<ParakeetModel>>,
}

impl Model {
    pub fn new(models_root: &Path) -> Self {
        let dir = models_root.join(MODEL_NAME);
        let status = if dir.join(COMPLETE_MARKER).exists() { ModelStatus::Loading } else { ModelStatus::NotDownloaded };
        Self { dir, status: Mutex::new(status), engine: Mutex::new(None) }
    }

    pub fn status(&self) -> ModelStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn is_downloaded(&self) -> bool {
        self.dir.join(COMPLETE_MARKER).exists()
    }

    fn set_status(&self, app: &AppHandle, status: ModelStatus) {
        *self.status.lock().unwrap() = status.clone();
        let _ = app.emit("model-status", status);
    }

    /// Loads the model into memory (blocking; call from a background thread).
    pub fn load(&self, app: &AppHandle) {
        self.set_status(app, ModelStatus::Loading);
        let mut engine = self.engine.lock().unwrap();
        let started = std::time::Instant::now();
        match ParakeetModel::load(&self.dir, &Quantization::Int8) {
            Ok(mut m) => {
                // Warm-up run so the first real dictation isn't slower.
                let _ = m.transcribe_with(&vec![0.0; 8000], &ParakeetParams::default());
                *engine = Some(m);
                drop(engine);
                log::info!("model loaded in {:?}", started.elapsed());
                self.set_status(app, ModelStatus::Ready);
            }
            Err(e) => {
                drop(engine);
                log::error!("model load failed: {e}");
                self.set_status(app, ModelStatus::Error { message: format!("Couldn't load the speech model: {e}") });
            }
        }
    }

    /// Transcribes 16 kHz mono samples. Blocks until the model is loaded.
    pub fn transcribe(&self, samples: &[f32]) -> Result<String, String> {
        let mut engine = self.engine.lock().unwrap();
        let model = engine.as_mut().ok_or("The speech model isn't ready yet")?;
        model
            .transcribe_with(samples, &ParakeetParams::default())
            .map(|r| r.text)
            .map_err(|e| format!("Transcription failed: {e}"))
    }

    /// Like `transcribe`, but gives up instead of waiting if the model is busy
    /// or still loading, so the live preview never delays a real dictation.
    pub fn transcribe_preview(&self, samples: &[f32]) -> Option<String> {
        let mut engine = self.engine.try_lock().ok()?;
        let model = engine.as_mut()?;
        model.transcribe_with(samples, &ParakeetParams::default()).ok().map(|r| r.text)
    }

    pub async fn download(&self, app: &AppHandle) -> Result<(), String> {
        if matches!(self.status(), ModelStatus::Downloading { .. }) {
            return Ok(());
        }
        let result = self.download_files(app).await;
        if let Err(e) = &result {
            log::error!("model download failed: {e}");
            self.set_status(app, ModelStatus::Error { message: e.clone() });
        }
        result
    }

    async fn download_files(&self, app: &AppHandle) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let client = reqwest::Client::new();

        // Sum file sizes up front so progress is one continuous bar.
        let mut sizes = Vec::new();
        for file in FILES {
            let resp = client.head(format!("{BASE_URL}/{file}")).send().await.map_err(net_err)?;
            sizes.push(resp.content_length().unwrap_or(0));
        }
        let total: u64 = sizes.iter().sum();
        let mut downloaded = 0u64;
        self.set_status(app, ModelStatus::Downloading { downloaded, total });

        for (file, size) in FILES.iter().zip(&sizes) {
            let dest = self.dir.join(file);
            if dest.metadata().map(|m| m.len() == *size && *size > 0).unwrap_or(false) {
                downloaded += size;
                continue;
            }
            let part = self.dir.join(format!("{file}.part"));
            let done_before = downloaded;
            // Large downloads get reset mid-stream fairly often; resume from
            // where the partial file left off instead of starting over.
            let mut attempt = 0;
            loop {
                let have = part.metadata().map(|m| m.len()).unwrap_or(0);
                downloaded = done_before + have;
                match self.download_file(app, &client, file, &part, have, *size, &mut downloaded, total).await {
                    Ok(()) => break,
                    Err(e) if attempt < MAX_RETRIES => {
                        attempt += 1;
                        log::warn!("download of {file} interrupted ({e}), retry {attempt}/{MAX_RETRIES}");
                        backoff(attempt).await;
                    }
                    Err(e) => return Err(e),
                }
            }
            std::fs::rename(&part, &dest).map_err(|e| e.to_string())?;
        }
        std::fs::write(self.dir.join(COMPLETE_MARKER), b"").map_err(|e| e.to_string())?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn download_file(
        &self,
        app: &AppHandle,
        client: &reqwest::Client,
        file: &str,
        part: &Path,
        offset: u64,
        expected: u64,
        downloaded: &mut u64,
        total: u64,
    ) -> Result<(), String> {
        let mut req = client.get(format!("{BASE_URL}/{file}"));
        if offset > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={offset}-"));
        }
        let resp = req.send().await.and_then(|r| r.error_for_status()).map_err(net_err)?;
        let resumed = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        let mut out = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .open(part)
            .map_err(|e| e.to_string())?;
        if !resumed {
            *downloaded -= offset; // server ignored the range; starting over
        }
        let mut stream = resp.bytes_stream();
        let mut last_emit = std::time::Instant::now();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(net_err)?;
            out.write_all(&chunk).map_err(|e| e.to_string())?;
            *downloaded += chunk.len() as u64;
            if last_emit.elapsed().as_millis() > 100 {
                self.set_status(app, ModelStatus::Downloading { downloaded: *downloaded, total });
                last_emit = std::time::Instant::now();
            }
        }
        drop(out);
        let len = part.metadata().map(|m| m.len()).unwrap_or(0);
        if expected > 0 && len != expected {
            return Err(format!("connection closed early ({len} of {expected} bytes)"));
        }
        Ok(())
    }
}

const MAX_RETRIES: u32 = 5;

/// Waits 2, 4, 8, 16… seconds between retries.
async fn backoff(attempt: u32) {
    let secs = 2u64.pow(attempt.min(4));
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(std::time::Duration::from_secs(secs))).await;
}

fn net_err(e: reqwest::Error) -> String {
    format!("Download failed — check your internet connection ({e})")
}
