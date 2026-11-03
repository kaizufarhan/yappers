//! Speech model download and lifecycle. The model is fetched on first run
//! rather than bundled, keeping the installer small.

use futures_util::StreamExt;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
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
    /// Set for the whole download, including the size lookups before the
    /// first progress event, so a second click can't start a parallel copy.
    downloading: AtomicBool,
}

impl Model {
    pub fn new(models_root: &Path) -> Self {
        let dir = models_root.join(MODEL_NAME);
        let status = if dir.join(COMPLETE_MARKER).exists() { ModelStatus::Loading } else { ModelStatus::NotDownloaded };
        Self { dir, status: Mutex::new(status), engine: Mutex::new(None), downloading: AtomicBool::new(false) }
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
        if self.downloading.swap(true, Ordering::SeqCst) {
            // Not shown to the user; the running download keeps reporting
            // through model-status.
            return Err("The speech model is already downloading".into());
        }
        let result = self.download_files(app).await;
        self.downloading.store(false, Ordering::SeqCst);
        match &result {
            Ok(()) => log::info!("model download complete"),
            Err(e) => {
                log::error!("model download failed: {e}");
                self.set_status(app, ModelStatus::Error { message: e.clone() });
            }
        }
        result
    }

    async fn download_files(&self, app: &AppHandle) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        // Show the bar straight away; the size lookups below can take a while.
        self.set_status(app, ModelStatus::Downloading { downloaded: 0, total: 0 });
        // No overall timeout: on a slow link the encoder alone takes many
        // minutes. A connection that goes silent (sleep, Wi-Fi switch) fails
        // the read instead, and the retry below resumes where it stopped.
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .tcp_keepalive(READ_TIMEOUT)
            .build()
            .map_err(net_err)?;

        // Sum file sizes up front so progress is one continuous bar.
        let mut sizes = Vec::new();
        for file in FILES {
            let resp = client
                .head(format!("{BASE_URL}/{file}"))
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(net_err)?;
            sizes.push(header_length(&resp).unwrap_or(0));
        }
        // One unknown size would make the bar overshoot, so treat it as unknown.
        let total: u64 = if sizes.contains(&0) { 0 } else { sizes.iter().sum() };
        log::info!("downloading model: sizes {sizes:?}, total {total} bytes");
        let mut downloaded = 0u64;
        self.set_status(app, ModelStatus::Downloading { downloaded, total });

        for (file, &size) in FILES.iter().zip(&sizes) {
            let dest = self.dir.join(file);
            if size > 0 && dest.metadata().map(|m| m.len() == size).unwrap_or(false) {
                downloaded += size;
                continue;
            }
            let part = self.dir.join(format!("{file}.part"));
            let done_before = downloaded;
            // Large downloads get reset mid-stream fairly often; resume from
            // where the partial file left off instead of starting over.
            let mut attempt = 0;
            loop {
                let mut have = part.metadata().map(|m| m.len()).unwrap_or(0);
                if size > 0 && have > size {
                    log::warn!("{file}.part is larger than the file ({have} of {size} bytes), starting over");
                    let _ = std::fs::remove_file(&part);
                    have = 0;
                }
                downloaded = done_before + have;
                if size > 0 && have == size {
                    break; // finished last time but never moved into place
                }
                match self.download_file(app, &client, file, &part, have, size, &mut downloaded, total).await {
                    Ok(()) => break,
                    Err(e) => {
                        // A drop after real progress shouldn't use up the retries
                        // meant for a connection that can't get going at all.
                        if part.metadata().map(|m| m.len()).unwrap_or(0) > have {
                            attempt = 0;
                        }
                        if attempt >= MAX_RETRIES {
                            return Err(e);
                        }
                        attempt += 1;
                        log::warn!("download of {file} interrupted ({e}), retry {attempt}/{MAX_RETRIES}");
                        backoff(attempt).await;
                    }
                }
            }
            rename_with_retry(&part, &dest).await?;
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
        let resp = req.send().await.map_err(net_err)?;
        let status = resp.status();
        log::info!(
            "{file}: HTTP {status} from {} at byte {offset}, {:?} bytes in response",
            resp.url().host_str().unwrap_or("?"),
            header_length(&resp)
        );
        if offset > 0 && status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            return Ok(()); // the partial file already holds everything
        }
        let resp = resp.error_for_status().map_err(net_err)?;
        let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
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
        self.set_status(app, ModelStatus::Downloading { downloaded: *downloaded, total });
        let mut stream = resp.bytes_stream();
        let mut last_emit = std::time::Instant::now();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(net_err)?;
            out.write_all(&chunk).map_err(|e| e.to_string())?;
            let before = *downloaded;
            *downloaded += chunk.len() as u64;
            if total > 0 && before * 10 / total < *downloaded * 10 / total {
                log::info!("model download at {}% ({} of {total} bytes)", *downloaded * 100 / total, *downloaded);
            }
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
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long the server may send nothing before the download counts as stalled.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// The Content-Length header. reqwest's `content_length()` can't be used for
/// this: it reports the body it received, which for a HEAD is always 0.
fn header_length(resp: &reqwest::Response) -> Option<u64> {
    resp.headers().get(reqwest::header::CONTENT_LENGTH)?.to_str().ok()?.parse().ok()
}

/// Waits 2, 4, 8, 16… seconds between retries.
async fn backoff(attempt: u32) {
    sleep(Duration::from_secs(2u64.pow(attempt.min(4)))).await;
}

async fn sleep(duration: Duration) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(duration)).await;
}

/// Antivirus often holds a freshly written file open for a moment to scan it,
/// which makes the rename fail with "access denied".
async fn rename_with_retry(from: &Path, to: &Path) -> Result<(), String> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if attempt < MAX_RETRIES => {
                attempt += 1;
                log::warn!("couldn't move {} into place ({e}), retry {attempt}/{MAX_RETRIES}", to.display());
                sleep(Duration::from_secs(1)).await;
            }
            Err(e) => return Err(format!("Couldn't save the speech model ({e})")),
        }
    }
}

fn net_err(e: reqwest::Error) -> String {
    if e.is_timeout() {
        return format!(
            "Download stalled — no data from the server for {} seconds. Check your internet connection",
            READ_TIMEOUT.as_secs()
        );
    }
    // The cause chain holds the useful part, such as a certificate error
    // behind a company proxy. The URL is dropped: past the CDN redirect it's
    // a signed link.
    let e = e.without_url();
    let mut detail = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(s) = source {
        detail.push_str(": ");
        detail.push_str(&s.to_string());
        source = s.source();
    }
    format!("Download failed — check your internet connection ({detail})")
}
