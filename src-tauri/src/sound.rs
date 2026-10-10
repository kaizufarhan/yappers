//! Short Windows playback cues. WAV data is embedded so installed builds work
//! without separate asset paths or webview autoplay permissions.

use windows::core::PCWSTR;
use windows::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_SYNC};
use crate::settings::SoundStyle;

pub enum Cue {
    Start,
    Stop,
}

fn bytes(style: SoundStyle, cue: Cue) -> &'static [u8] {
    match (style, cue) {
        (SoundStyle::Airy, Cue::Start) => include_bytes!("../sounds/airy-start.wav"),
        (SoundStyle::Airy, Cue::Stop) => include_bytes!("../sounds/airy-stop.wav"),
        (SoundStyle::Glass, Cue::Start) => include_bytes!("../sounds/glass-start.wav"),
        (SoundStyle::Glass, Cue::Stop) => include_bytes!("../sounds/glass-stop.wav"),
        (SoundStyle::Whoosh, Cue::Start) => include_bytes!("../sounds/whoosh-start.wav"),
        (SoundStyle::Whoosh, Cue::Stop) => include_bytes!("../sounds/whoosh-stop.wav"),
    }
}

fn play_blocking(style: SoundStyle, cue: Cue) {
    let bytes = bytes(style, cue);
    // PlaySoundW takes a PCWSTR even with SND_MEMORY, where the pointer is
    // interpreted as WAV bytes. Copy to u16 storage for proper alignment.
    let aligned: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
    let played = unsafe { PlaySoundW(PCWSTR(aligned.as_ptr()), None, SND_MEMORY | SND_NODEFAULT | SND_SYNC) };
    if !played.as_bool() {
        log::warn!("couldn't play dictation sound");
    }
}

pub fn play(style: SoundStyle, cue: Cue) {
    std::thread::spawn(move || play_blocking(style, cue));
}

pub fn preview(style: SoundStyle) {
    std::thread::spawn(move || {
        play_blocking(style, Cue::Start);
        std::thread::sleep(std::time::Duration::from_millis(220));
        play_blocking(style, Cue::Stop);
    });
}
