//! The dictation state machine. Turns raw hotkey events into gestures:
//!
//! * **Hold** the combo: records while held, transcribes on release.
//! * **Double-tap** the combo: hands-free mode; press once more to finish.
//! * A single short tap does nothing (the recording is discarded).
//! * Esc, or using the combo as part of another shortcut, cancels.

use crate::hotkey::{self, HotkeyEvent, RECORDING};
use crate::{audio, cleanup, overlay, paste, sound, Core};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

/// Presses shorter than this count as taps rather than holds.
const HOLD_THRESHOLD: Duration = Duration::from_millis(300);
/// Max gap between the two taps of a double-tap.
const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(400);
/// Clips shorter than this are treated as accidental.
const MIN_SAMPLES: usize = audio::TARGET_RATE as usize / 4;
/// Clips quieter than this peak level contain no speech.
const SILENCE_PEAK: f32 = 0.01;
/// The live preview re-transcribes this much recent audio on each pass.
/// Longer windows give the model more context but cost more per pass.
const PREVIEW_WINDOW_SECS: f32 = 4.0;
/// Minimum time between preview passes; slower machines just update less often.
const PREVIEW_INTERVAL: Duration = Duration::from_millis(200);

pub enum Command {
    Hotkey(HotkeyEvent),
    /// Finish button on the overlay (hands-free mode).
    Stop,
    /// Cancel button on the overlay.
    Cancel,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum OverlayState {
    Recording,
    Locked,
    Processing,
    Error { message: String },
    Hidden,
}

#[derive(Clone, Serialize)]
pub struct OverlaySnapshot {
    pub revision: u64,
    pub state: OverlayState,
}

static OVERLAY_SNAPSHOT: Mutex<OverlaySnapshot> = Mutex::new(OverlaySnapshot {
    revision: 0,
    state: OverlayState::Hidden,
});

pub fn overlay_snapshot() -> OverlaySnapshot {
    OVERLAY_SNAPSHOT.lock().unwrap().clone()
}

enum Phase {
    Idle,
    Holding { since: Instant, visible: bool },
    AwaitingSecondTap { deadline: Instant },
    SecondTapPressed { since: Instant },
    Locked,
    LockedStopPressed,
}

struct Session {
    recording: audio::Recording,
    started: Instant,
    /// Window that had focus when recording began; paste goes back there.
    target_window: isize,
    announced: bool,
    /// Stops the live-preview thread.
    preview_stop: Arc<AtomicBool>,
}

#[derive(Clone, Serialize)]
struct LiveText {
    session: u64,
    text: String,
}

/// Bumped on every new recording so stale "hide overlay" timers are ignored.
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn spawn(core: Arc<Core>, rx: mpsc::Receiver<Command>) {
    std::thread::Builder::new()
        .name("yappers-dictation".into())
        .spawn(move || run(core, rx))
        .expect("spawn dictation thread");
}

fn run(core: Arc<Core>, rx: mpsc::Receiver<Command>) {
    let mut phase = Phase::Idle;
    let mut session: Option<Session> = None;

    loop {
        let cmd = match phase {
            Phase::Holding { since, visible: false } | Phase::SecondTapPressed { since } => {
                match rx.recv_timeout((since + HOLD_THRESHOLD).saturating_duration_since(Instant::now())) {
                    Ok(cmd) => Some(cmd),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            Phase::AwaitingSecondTap { deadline } => {
                match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(cmd) => Some(cmd),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            _ => match rx.recv() {
                Ok(cmd) => Some(cmd),
                Err(_) => return,
            },
        };

        phase = match (phase, cmd) {
            // Single tap with no follow-up: discard.
            (Phase::AwaitingSecondTap { .. }, None) => {
                cancel(&core, &mut session);
                Phase::Idle
            }
            (Phase::Holding { since, visible: false }, None) => {
                announce(&core, &mut session, OverlayState::Recording);
                Phase::Holding { since, visible: true }
            }
            (Phase::SecondTapPressed { since }, None) => {
                announce(&core, &mut session, OverlayState::Recording);
                Phase::Holding { since, visible: true }
            }
            (Phase::Idle, Some(Command::Hotkey(HotkeyEvent::Down))) => match begin(&core) {
                Ok(s) => {
                    session = Some(s);
                    Phase::Holding { since: Instant::now(), visible: false }
                }
                Err(message) => {
                    hotkey::release_gesture();
                    show_error(&core, message);
                    Phase::Idle
                }
            },
            (Phase::Holding { since, .. }, Some(Command::Hotkey(HotkeyEvent::Up))) => {
                if since.elapsed() >= HOLD_THRESHOLD {
                    finish(&core, &mut session);
                    Phase::Idle
                } else {
                    Phase::AwaitingSecondTap { deadline: Instant::now() + DOUBLE_TAP_WINDOW }
                }
            }
            (Phase::AwaitingSecondTap { .. }, Some(Command::Hotkey(HotkeyEvent::Down))) => {
                Phase::SecondTapPressed { since: Instant::now() }
            }
            (Phase::SecondTapPressed { since }, Some(Command::Hotkey(HotkeyEvent::Up))) => {
                if since.elapsed() >= HOLD_THRESHOLD {
                    finish(&core, &mut session);
                    Phase::Idle
                } else {
                    announce(&core, &mut session, OverlayState::Locked);
                    Phase::Locked
                }
            }
            (Phase::Locked, Some(Command::Hotkey(HotkeyEvent::Down))) => Phase::LockedStopPressed,
            (Phase::LockedStopPressed, Some(Command::Hotkey(HotkeyEvent::Up)))
            | (Phase::Locked, Some(Command::Stop))
            | (Phase::LockedStopPressed, Some(Command::Stop)) => {
                finish(&core, &mut session);
                Phase::Idle
            }
            (Phase::LockedStopPressed, Some(Command::Hotkey(HotkeyEvent::Interrupted))) => Phase::Locked,
            (
                Phase::Holding { .. } | Phase::AwaitingSecondTap { .. } | Phase::SecondTapPressed { .. },
                Some(Command::Hotkey(HotkeyEvent::Interrupted)),
            ) => {
                cancel(&core, &mut session);
                Phase::Idle
            }
            (
                Phase::Holding { .. } | Phase::AwaitingSecondTap { .. } | Phase::SecondTapPressed { .. }
                | Phase::Locked | Phase::LockedStopPressed,
                Some(Command::Hotkey(HotkeyEvent::Escape)) | Some(Command::Cancel),
            ) => {
                cancel(&core, &mut session);
                Phase::Idle
            }
            (phase, Some(Command::Hotkey(HotkeyEvent::CaptureProgress(keys)))) => {
                emit_keys(&core, "hotkey-capture-progress", &keys);
                phase
            }
            (phase, Some(Command::Hotkey(HotkeyEvent::Captured(keys)))) => {
                emit_keys(&core, "hotkey-captured", &keys);
                phase
            }
            (phase, _) => phase,
        };
    }
}

fn begin(core: &Arc<Core>) -> Result<Session, String> {
    let target_window = paste::foreground_window();
    let mic = core.settings.lock().unwrap().microphone.clone();
    let recording = audio::start(mic.as_deref())?;
    let session = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    RECORDING.store(true, Ordering::SeqCst);
    set_overlay(core, OverlayState::Hidden);
    overlay::hide(&core.app);
    let preview_stop = Arc::new(AtomicBool::new(false));
    spawn_preview(core.clone(), recording.tap(), session, preview_stop.clone());
    Ok(Session { recording, started: Instant::now(), target_window, announced: false, preview_stop })
}

/// Streams a rough transcript of the last few seconds to the overlay while
/// recording, so words appear as they're spoken. The pasted text still comes
/// from one accurate pass over the whole recording in `finish`. (Transcribing
/// long dictations in pieces at pauses was tried and removed: the model ends
/// every piece with a full stop, so mid-sentence cuts couldn't be told apart
/// from real sentence ends, and words near the cuts lost accuracy.)
fn spawn_preview(core: Arc<Core>, tap: audio::Tap, session: u64, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut last = String::new();
        let mut previous_pass: Vec<String> = Vec::new();
        while !stop.load(Ordering::SeqCst) {
            let started = Instant::now();
            let samples = tap.tail(PREVIEW_WINDOW_SECS);
            if samples.len() >= MIN_SAMPLES && audio::peak(&samples) >= SILENCE_PEAK {
                if let Some(raw) = core.model.transcribe_preview(&samples) {
                    let words: Vec<&str> = raw.split_whitespace().collect();
                    let normalized: Vec<String> = words.iter().map(|w| normalize_word(w)).collect();
                    // The window ends mid-word while you're talking, and the
                    // model guesses how that last word finishes. Hold back just
                    // that word until a second pass agrees on it, or speech
                    // pauses (then it's complete). Everything before it is
                    // shown right away.
                    let shown = if trailing_silence(&samples) {
                        words.len()
                    } else {
                        confirmed_words(&previous_pass, &normalized).max(words.len().saturating_sub(1))
                    };
                    previous_pass = normalized;

                    let fillers = {
                        let settings = core.settings.lock().unwrap();
                        settings.remove_fillers.then(|| settings.filler_words.clone())
                    };
                    let text = cleanup::clean(&words[..shown].join(" "), fillers.as_deref());
                    if !stop.load(Ordering::SeqCst) && !text.is_empty() && text != last {
                        let _ = core.app.emit_to(overlay::LABEL, "live-text", LiveText { session, text: text.clone() });
                        last = text;
                    }
                }
            }
            std::thread::sleep(PREVIEW_INTERVAL.saturating_sub(started.elapsed()));
        }
    });
}

fn normalize_word(word: &str) -> String {
    word.chars().filter(|c| c.is_alphanumeric() || *c == '\'').flat_map(char::to_lowercase).collect()
}

/// How many leading words of the current pass the previous pass agrees on.
/// The window slides between passes, so the current words are matched against
/// any position in the previous ones; the first word may be cut off at the
/// window's start, so a run may also begin at the second word.
fn confirmed_words(previous: &[String], current: &[String]) -> usize {
    let mut best = 0;
    for skip in 0..current.len().min(2) {
        for offset in 0..previous.len() {
            let run = previous[offset..].iter().zip(&current[skip..]).take_while(|(a, b)| a == b).count();
            if run > 0 {
                best = best.max(skip + run);
            }
        }
    }
    best
}

/// True when the last ~0.3 s of audio is quiet, i.e. the speaker paused.
fn trailing_silence(samples: &[f32]) -> bool {
    let tail = &samples[samples.len().saturating_sub(audio::TARGET_RATE as usize * 3 / 10)..];
    audio::peak(tail) < SILENCE_PEAK * 2.0
}

fn announce(core: &Arc<Core>, session: &mut Option<Session>, state: OverlayState) {
    if let Some(s) = session.as_mut() {
        s.announced = true;
        set_overlay(core, state);
        overlay::show(&core.app, s.target_window);
        let settings = core.settings.lock().unwrap();
        if settings.sound_cues {
            sound::play(settings.sound_style, sound::Cue::Start);
        }
    }
}

fn cancel(core: &Arc<Core>, session: &mut Option<Session>) {
    RECORDING.store(false, Ordering::SeqCst);
    hotkey::release_gesture();
    if let Some(s) = session.take() {
        s.preview_stop.store(true, Ordering::SeqCst);
        let settings = core.settings.lock().unwrap();
        if s.announced && settings.sound_cues {
            sound::play(settings.sound_style, sound::Cue::Stop);
        }
        drop(settings);
        drop(s.recording.finish());
    }
    set_overlay(core, OverlayState::Hidden);
    overlay::hide(&core.app);
}

/// Stops recording and hands the audio to a worker thread, so the hotkey
/// stays responsive while the model runs.
fn finish(core: &Arc<Core>, session: &mut Option<Session>) {
    RECORDING.store(false, Ordering::SeqCst);
    hotkey::release_gesture();
    let Some(s) = session.take() else { return };
    s.preview_stop.store(true, Ordering::SeqCst);
    let duration_ms = s.started.elapsed().as_millis() as i64;
    let samples = s.recording.finish();
    let settings = core.settings.lock().unwrap();
    if s.announced && settings.sound_cues {
        sound::play(settings.sound_style, sound::Cue::Stop);
    }
    drop(settings);

    if samples.len() < MIN_SAMPLES || audio::peak(&samples) < SILENCE_PEAK {
        log::info!("discarding clip: {} samples, peak {:.4}", samples.len(), audio::peak(&samples));
        overlay::hide(&core.app);
        set_overlay(core, OverlayState::Hidden);
        return;
    }

    // Keep the overlay visible until transcription and paste have
    // actually finished. A short hold may not have shown the recording state.
    set_overlay(core, OverlayState::Processing);
    overlay::show(&core.app, s.target_window);
    let core = core.clone();
    let generation = GENERATION.load(Ordering::SeqCst);
    let target = s.target_window;
    std::thread::spawn(move || {
        handle_result(&core, generation, transcribe_and_insert(&core, &samples, duration_ms, target));
    });
}

fn handle_result(core: &Arc<Core>, generation: u64, result: Result<bool, String>) {
    match result {
        Ok(_) => {
            if GENERATION.load(Ordering::SeqCst) == generation && !RECORDING.load(Ordering::SeqCst) {
                overlay::hide(&core.app);
                set_overlay(core, OverlayState::Hidden);
            }
        }
        Err(message) => {
            log::warn!("dictation failed: {message}");
            if GENERATION.load(Ordering::SeqCst) == generation && !RECORDING.load(Ordering::SeqCst) {
                show_error(core, message);
            }
        }
    }
}

/// Hides the overlay after `delay`, unless a newer recording has started.
fn hide_later(core: &Arc<Core>, delay: Duration) {
    let core = core.clone();
    let generation = GENERATION.load(Ordering::SeqCst);
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        if GENERATION.load(Ordering::SeqCst) == generation && !RECORDING.load(Ordering::SeqCst) {
            set_overlay(&core, OverlayState::Hidden);
            overlay::hide(&core.app);
        }
    });
}

/// Returns Ok(false) when there was nothing to insert.
fn transcribe_and_insert(core: &Arc<Core>, samples: &[f32], duration_ms: i64, target: isize) -> Result<bool, String> {
    if !core.model.is_downloaded() {
        show_main_window(core);
        return Err("Download the model in Yappers first".into());
    }
    let started = Instant::now();
    let raw = core.model.transcribe(samples)?;
    log::info!("transcribed {:.1}s of audio in {:?}", samples.len() as f32 / 16000.0, started.elapsed());

    let settings = core.settings.lock().unwrap().clone();
    let fillers = settings.remove_fillers.then_some(settings.filler_words.as_slice());
    let text = cleanup::clean(&raw, fillers);
    if text.is_empty() {
        return Ok(false);
    }
    let insert = if settings.trailing_space { format!("{text} ") } else { text.clone() };

    let pasted = if settings.paste_into_app {
        match paste::paste(&insert, target) {
            Ok(()) => true,
            Err(e) => {
                log::warn!("{e}");
                false
            }
        }
    } else {
        paste::copy_to_clipboard(&text)?;
        false
    };

    match core.history.lock().unwrap().add(&text, &raw, duration_ms, pasted) {
        Ok(entry) => {
            let _ = core.app.emit("history-added", entry);
        }
        Err(e) => log::error!("failed to save history: {e}"),
    }
    Ok(true)
}

fn emit_keys(core: &Arc<Core>, event: &str, keys: &[u32]) {
    let labels: Vec<String> = keys.iter().map(|&k| crate::settings::key_label(k)).collect();
    let _ = core.app.emit(event, serde_json::json!({ "keys": keys, "labels": labels }));
}

fn set_overlay(core: &Arc<Core>, state: OverlayState) {
    let snapshot = {
        let mut snapshot = OVERLAY_SNAPSHOT.lock().unwrap();
        snapshot.revision += 1;
        snapshot.state = state;
        snapshot.clone()
    };
    let _ = core.app.emit_to(overlay::LABEL, "overlay-state", snapshot);
}

fn show_error(core: &Arc<Core>, message: String) {
    set_overlay(core, OverlayState::Error { message });
    overlay::show(&core.app, paste::foreground_window());
    hide_later(core, Duration::from_millis(2500));
}

pub fn show_main_window(core: &Arc<Core>) {
    if let Some(w) = core.app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::confirmed_words;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn holds_back_guessed_final_word() {
        // Previous pass heard "...what i'm sa"; the model guessed "same".
        let prev = words("so i think that's what i'm same");
        let cur = words("i think that's what i'm saying");
        assert_eq!(confirmed_words(&prev, &cur), 5); // up to "i'm"
    }

    #[test]
    fn tolerates_cut_off_first_word_and_sliding_window() {
        let prev = words("think that's what i'm saying");
        let cur = words("nk that's what i'm saying right");
        assert_eq!(confirmed_words(&prev, &cur), 5);
    }

    #[test]
    fn first_pass_shows_nothing() {
        assert_eq!(confirmed_words(&[], &words("hello there")), 0);
    }
}
