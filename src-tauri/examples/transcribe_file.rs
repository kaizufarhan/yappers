//! Dev tool: transcribe a 16 kHz mono 16-bit WAV with the same model and
//! cleanup the app uses, and print timings.
//!
//! cargo run --release --example transcribe_file -- <model_dir> <file.wav>

#[path = "../src/cleanup.rs"]
mod cleanup;

use std::path::PathBuf;
use std::time::Instant;
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams};
use transcribe_rs::onnx::Quantization;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (model_dir, wav) = match (args.get(1), args.get(2)) {
        (Some(m), Some(w)) => (PathBuf::from(m), PathBuf::from(w)),
        _ => {
            eprintln!("usage: transcribe_file <model_dir> <file.wav>");
            std::process::exit(2);
        }
    };

    // YAPPERS_ACCEL=dml runs on the GPU via DirectML (needs the ort-directml feature).
    if std::env::var("YAPPERS_ACCEL").as_deref() == Ok("dml") {
        transcribe_rs::set_ort_accelerator(transcribe_rs::OrtAccelerator::DirectMl);
        println!("accel:      DirectML");
    }

    let t = Instant::now();
    let mut model = ParakeetModel::load(&model_dir, &Quantization::Int8).expect("load model");
    println!("load:       {:?}", t.elapsed());

    let samples = transcribe_rs::audio::read_wav_samples(&wav).expect("read wav");
    let secs = samples.len() as f32 / 16000.0;

    let t = Instant::now();
    let raw = model.transcribe_with(&samples, &ParakeetParams::default()).expect("transcribe").text;
    let cold = t.elapsed();
    let t = Instant::now();
    let _ = model.transcribe_with(&samples, &ParakeetParams::default()).expect("transcribe");
    let took = t.elapsed();
    println!("first run:  {cold:?}");
    // Real dictations all differ in length; check other lengths stay fast.
    for frac in [0.17, 0.43, 0.71] {
        let part = &samples[..(samples.len() as f32 * frac) as usize];
        let t = Instant::now();
        let _ = model.transcribe_with(part, &ParakeetParams::default()).expect("transcribe");
        println!("{:>5.1}s clip: {:?}", part.len() as f32 / 16000.0, t.elapsed());
    }
    println!("audio:      {secs:.1}s");
    println!("transcribe: {took:?} ({:.0}x real-time)", secs / took.as_secs_f32());
    println!("raw:        {raw}");
    println!("cleaned:    {}", cleanup::clean(&raw, Some(&cleanup::default_fillers())));
}
