//! Microphone capture. Each recording runs on its own thread that owns the cpal
//! stream (streams aren't `Send` on every platform); audio is downmixed to mono
//! while recording and resampled to 16 kHz when it stops.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

pub const TARGET_RATE: u32 = 16_000;

pub struct Recording {
    stop_tx: mpsc::Sender<()>,
    handle: JoinHandle<Vec<f32>>,
    tap: Tap,
}

/// Read access to a recording in progress, for the live preview.
#[derive(Clone)]
pub struct Tap {
    buffer: Arc<Mutex<Vec<f32>>>,
    rate: u32,
}

impl Tap {
    /// The last `secs` seconds recorded so far, as 16 kHz mono.
    pub fn tail(&self, secs: f32) -> Vec<f32> {
        let take = (secs * self.rate as f32) as usize;
        let recent = {
            let buffer = self.buffer.lock().unwrap();
            buffer[buffer.len().saturating_sub(take)..].to_vec()
        };
        resample(&recent, self.rate, TARGET_RATE)
    }
}

impl Recording {
    pub fn tap(&self) -> Tap {
        self.tap.clone()
    }

    /// Stops the stream and returns 16 kHz mono samples.
    pub fn finish(self) -> Vec<f32> {
        let _ = self.stop_tx.send(());
        self.handle.join().unwrap_or_default()
    }
}

pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|devs| devs.filter_map(|d| d.description().ok().map(|x| x.name().to_string())).collect())
        .unwrap_or_default()
}

fn find_device(name: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = name {
        if let Ok(mut devs) = host.input_devices() {
            if let Some(d) = devs.find(|d| d.description().map(|x| x.name() == name).unwrap_or(false)) {
                return Some(d);
            }
        }
        log::warn!("microphone '{name}' not found, falling back to default");
    }
    host.default_input_device()
}

/// Starts recording from the named microphone, or the default one.
pub fn start(device_name: Option<&str>) -> Result<Recording, String> {
    let device = find_device(device_name).ok_or("No microphone found")?;
    let config = device.default_input_config().map_err(|e| format!("Microphone unavailable: {e}"))?;
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let rate = config.sample_rate();
    let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(rate as usize * 30)));
    let tap = Tap { buffer: buffer.clone(), rate };

    let handle = std::thread::spawn(move || {
        let channels = config.channels() as usize;
        let format = config.sample_format();
        let stream = build_stream(&device, config.into(), format, channels, buffer.clone());
        let stream = match stream.and_then(|s| s.play().map(|_| s).map_err(|e| e.to_string())) {
            Ok(s) => {
                let _ = ready_tx.send(Ok(()));
                s
            }
            Err(e) => {
                let _ = ready_tx.send(Err(format!("Couldn't open microphone: {e}")));
                return Vec::new();
            }
        };

        let _ = stop_rx.recv();
        drop(stream);
        let samples = std::mem::take(&mut *buffer.lock().unwrap());
        resample(&samples, rate, TARGET_RATE)
    });

    match ready_rx.recv() {
        Ok(Ok(())) => Ok(Recording { stop_tx, handle, tap }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("Microphone thread exited unexpectedly".into()),
    }
}

fn build_stream(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    format: SampleFormat,
    channels: usize,
    buffer: Arc<Mutex<Vec<f32>>>,
) -> Result<cpal::Stream, String> {
    let err_fn = |e: cpal::Error| log::warn!("audio stream error: {e}");
    macro_rules! build {
        ($t:ty) => {
            device.build_input_stream(
                config,
                move |data: &[$t], _: &_| {
                    let mono: Vec<f32> = data
                        .chunks(channels)
                        .map(|frame| frame.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / channels as f32)
                        .collect();
                    buffer.lock().unwrap().extend_from_slice(&mono);
                },
                err_fn,
                None,
            )
        };
    }
    match format {
        SampleFormat::F32 => build!(f32),
        SampleFormat::I16 => build!(i16),
        SampleFormat::I32 => build!(i32),
        SampleFormat::U16 => build!(u16),
        SampleFormat::U8 => build!(u8),
        other => return Err(format!("unsupported sample format {other}")),
    }
    .map_err(|e| e.to_string())
}

fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let out_len = (input.len() as f64 / ratio) as usize;
    let mut out = Vec::with_capacity(out_len);
    if ratio > 1.0 {
        // Downsampling: average each source window (a cheap low-pass filter).
        for i in 0..out_len {
            let start = (i as f64 * ratio) as usize;
            let end = (((i + 1) as f64 * ratio) as usize).min(input.len()).max(start + 1);
            out.push(input[start..end].iter().sum::<f32>() / (end - start) as f32);
        }
    } else {
        // Upsampling: linear interpolation.
        for i in 0..out_len {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = input[idx];
            let b = *input.get(idx + 1).unwrap_or(&a);
            out.push(a + (b - a) * frac);
        }
    }
    out
}

/// Peak loudness of a clip, used to skip transcribing silence.
pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}
