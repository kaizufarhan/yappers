//! Global hotkey detection via a Windows low-level keyboard hook.
//!
//! Standard global-shortcut APIs can't express "modifier-only" combos like
//! Ctrl+Alt, nor tell a hold apart from a double-tap, so we watch raw key
//! events and report when the configured combo goes down / up. Gesture logic
//! (hold vs double-tap) lives in `dictation.rs`.

use crate::settings::{VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, Instant};
use std::sync::{mpsc, OnceLock, RwLock};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY,
};
use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, SetTimer, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER,
};

const VK_ESCAPE: u32 = 0x1B;
/// Unassigned virtual key, sent to stop Alt/Win from opening menus on release.
const VK_MASK: u16 = 0xE8;
/// Keys held longer than this are re-checked against the real keyboard state.
/// Releases can go missing (Win+L, Ctrl+Alt+Del and UAC prompts swallow them),
/// which would otherwise leave a phantom key that blocks the combo forever.
const STALE_AFTER: Duration = Duration::from_secs(2);

/// Which input path owns the gesture in progress. While Yappers's own window is
/// focused its webview reports keys instead of the hook, but a gesture started
/// in another app must keep being tracked here even if focus moves to Yappers.
pub const OWNER_NONE: u8 = 0;
pub const OWNER_HOOK: u8 = 1;
pub const OWNER_WEBVIEW: u8 = 2;
pub static GESTURE_OWNER: AtomicU8 = AtomicU8::new(OWNER_NONE);

#[derive(Debug, Clone)]
pub enum HotkeyEvent {
    /// The configured combo is now fully held.
    Down,
    /// One of the combo keys was released.
    Up,
    /// Another key was pressed while the combo was held (e.g. Ctrl+Alt+Del).
    Interrupted,
    /// Escape was pressed while recording.
    Escape,
    /// Keys pressed so far while capturing a new combo (live feedback).
    CaptureProgress(Vec<u32>),
    /// A new combo was captured for the settings screen.
    Captured(Vec<u32>),
}

/// Tag placed in `dwExtraInfo` of every key event Yappers sends, so the hook
/// can tell our own synthetic input apart from real (or remapped) keys.
pub const INPUT_TAG: usize = 0x5941_5053; // "YAPS"

static SENDER: OnceLock<mpsc::Sender<HotkeyEvent>> = OnceLock::new();
static TARGET: RwLock<Vec<u32>> = RwLock::new(Vec::new());
static CAPTURING: AtomicBool = AtomicBool::new(false);
static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);
static RESET_GENERATION: AtomicU64 = AtomicU64::new(0);
pub static RECORDING: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct HookState {
    generation: u64,
    /// Keys currently held, with when they went down.
    down: Vec<(u32, Instant)>,
    active: bool,
    captured: Vec<u32>,
    capture_started: bool,
}

thread_local! {
    static STATE: RefCell<HookState> = RefCell::new(HookState::default());
}

pub fn set_target(keys: Vec<u32>) {
    *TARGET.write().unwrap() = keys;
    RESET_GENERATION.fetch_add(1, Ordering::SeqCst);
}

pub fn set_main_window(hwnd: isize) {
    MAIN_WINDOW.store(hwnd, Ordering::SeqCst);
}

pub fn is_capturing() -> bool {
    CAPTURING.load(Ordering::SeqCst)
}

pub fn set_capturing(on: bool) {
    log::info!("shortcut capture {}", if on { "started" } else { "stopped" });
    CAPTURING.store(on, Ordering::SeqCst);
    RESET_GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// How often the hook is replaced with a fresh one (see `start`).
const REINSTALL_EVERY_MS: u32 = 5_000;

/// Installs the hook on a dedicated thread with its own message loop.
///
/// Windows silently removes a low-level hook whose callback ever takes longer
/// than its timeout (about a second), e.g. when transcription has every CPU
/// core busy or around sleep/resume, and gives no notification. So:
/// * the thread runs at the highest priority, so it answers in time even when
///   the CPU is saturated, and
/// * the hook is replaced every few seconds, so even if Windows drops it the
///   shortcut recovers on its own instead of needing an app restart.
pub fn start(sender: mpsc::Sender<HotkeyEvent>) {
    let _ = SENDER.set(sender);
    std::thread::Builder::new()
        .name("yappers-hotkey".into())
        .spawn(|| unsafe {
            if let Err(e) = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL) {
                log::warn!("couldn't raise hotkey thread priority: {e}");
            }
            let Some(mut hook) = install_hook() else { return };
            // A thread timer: WM_TIMER arrives in this thread's message queue.
            SetTimer(None, 0, REINSTALL_EVERY_MS, None);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_TIMER {
                    // Hook callbacks only run while this loop waits for
                    // messages, so swapping here can't drop or double a key.
                    if let Some(fresh) = install_hook() {
                        let _ = UnhookWindowsHookEx(hook);
                        hook = fresh;
                    }
                    continue;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("spawn hotkey thread");
}

fn install_hook() -> Option<HHOOK> {
    match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) } {
        Ok(hook) => Some(hook),
        Err(e) => {
            log::error!("failed to install keyboard hook: {e}");
            None
        }
    }
}

fn normalize(vk: u32) -> u32 {
    match vk {
        0xA0 | 0xA1 => VK_SHIFT,
        0xA2 | 0xA3 => VK_CONTROL,
        0xA4 | 0xA5 => VK_MENU,
        0x5C => VK_LWIN,
        other => other,
    }
}

fn is_physically_down(vk: u32) -> bool {
    let check = |k: i32| unsafe { GetAsyncKeyState(k) } as u16 & 0x8000 != 0;
    match vk {
        VK_LWIN => check(0x5B) || check(0x5C),
        other => check(other as i32),
    }
}

/// Called when a dictation session ends, so either input path can start the next.
pub fn release_gesture() {
    GESTURE_OWNER.store(OWNER_NONE, Ordering::SeqCst);
}

fn send(event: HotkeyEvent) {
    if let Some(tx) = SENDER.get() {
        let _ = tx.send(event);
    }
}

fn send_mask_key() {
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(VK_MASK), dwFlags: flags, dwExtraInfo: INPUT_TAG, ..Default::default() },
        },
    };
    unsafe {
        SendInput(&[key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)], std::mem::size_of::<INPUT>() as i32);
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        // Ignore only input Yappers itself sent (the Ctrl+V paste). Other
        // synthetic input must still count: keyboard software like Logitech
        // G HUB or PowerToys re-sends remapped keys as injected events.
        if kb.dwExtraInfo != INPUT_TAG {
            // The focused Yappers webview reports its own keys, so the hook
            // keeps tracking state but stays quiet, unless it already owns a
            // gesture that started in another app.
            let defer = !CAPTURING.load(Ordering::SeqCst)
                && GESTURE_OWNER.load(Ordering::SeqCst) != OWNER_HOOK
                && GetForegroundWindow().0 as isize == MAIN_WINDOW.load(Ordering::SeqCst);
            let msg = wparam.0 as u32;
            let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
            let vk = normalize(kb.vkCode);
            if (is_down || is_up) && handle_key(vk, is_down, kb.flags.0 & LLKHF_INJECTED.0 != 0, defer) {
                return LRESULT(1); // swallow
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Returns true if the key event should be swallowed.
/// With `defer`, state is tracked but no gesture events are sent.
fn handle_key(vk: u32, is_down: bool, injected: bool, defer: bool) -> bool {
    STATE.with(|state| {
        let mut s = state.borrow_mut();
        let generation = RESET_GENERATION.load(Ordering::SeqCst);
        if s.generation != generation {
            s.generation = generation;
            s.down.clear();
            s.captured.clear();
            s.active = false;
            s.capture_started = false;
        }

        if CAPTURING.load(Ordering::SeqCst) {
            log::debug!("capture: key {vk:#04X} {} (injected: {injected})", if is_down { "down" } else { "up" });
            if !s.capture_started {
                s.capture_started = true;
                s.down.clear();
                s.captured.clear();
                s.active = false;
            }
            if is_down {
                if !s.down.iter().any(|&(k, _)| k == vk) {
                    s.down.push((vk, Instant::now()));
                }
                if !s.captured.contains(&vk) {
                    s.captured.push(vk);
                    let mut keys = s.captured.clone();
                    crate::settings::sort_keys(&mut keys);
                    send(HotkeyEvent::CaptureProgress(keys));
                }
            } else {
                s.down.retain(|&(k, _)| k != vk);
                if s.down.is_empty() && !s.captured.is_empty() {
                    let mut keys = std::mem::take(&mut s.captured);
                    crate::settings::sort_keys(&mut keys);
                    CAPTURING.store(false, Ordering::SeqCst);
                    RESET_GENERATION.fetch_add(1, Ordering::SeqCst);
                    s.capture_started = false;
                    send(HotkeyEvent::Captured(keys));
                }
            }
            // Let the focused settings webview see ordinary keys too. It can
            // capture them when another Windows hook prevents our global
            // callback from receiving a key. Win/Alt still need swallowing to
            // keep their system menus from opening during capture.
            return vk == VK_LWIN || vk == VK_MENU;
        }
        s.capture_started = false;

        // Trust down/up events for freshly pressed keys: the hook runs before
        // Windows updates its async key state, so querying it right away can
        // drop a modifier that is genuinely held. Only long-held keys are
        // re-checked, to clear releases that never arrived.
        s.down.retain(|&(k, at)| k == vk || at.elapsed() < STALE_AFTER || is_physically_down(k));

        let emit = |event: HotkeyEvent| {
            if !defer {
                send(event);
            }
        };
        let target = TARGET.read().unwrap();
        if is_down {
            let newly = !s.down.iter().any(|&(k, _)| k == vk);
            if newly {
                s.down.push((vk, Instant::now()));
            }
            if vk == VK_ESCAPE && RECORDING.load(Ordering::SeqCst) {
                emit(HotkeyEvent::Escape);
            }
            if newly && !target.is_empty() {
                if s.active {
                    s.active = false;
                    emit(HotkeyEvent::Interrupted);
                } else if s.down.len() == target.len() && target.iter().all(|k| s.down.iter().any(|&(d, _)| d == *k)) {
                    s.active = true;
                    // Even when deferring to the webview, stop Win/Alt from
                    // opening the Start menu or a menu bar on release.
                    if target.contains(&VK_MENU) || target.contains(&VK_LWIN) {
                        send_mask_key();
                    }
                    if !defer {
                        let _ = GESTURE_OWNER.compare_exchange(OWNER_NONE, OWNER_HOOK, Ordering::SeqCst, Ordering::SeqCst);
                    }
                    emit(HotkeyEvent::Down);
                }
            }
        } else {
            s.down.retain(|&(k, _)| k != vk);
            if s.active && target.contains(&vk) {
                s.active = false;
                emit(HotkeyEvent::Up);
            }
        }
        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_single_keys_combinations_and_capture() {
        let (tx, rx) = mpsc::channel();
        SENDER.set(tx).expect("hotkey sender should be initialized once");

        set_target(vec![VK_CONTROL]);
        handle_key(VK_CONTROL, true, false, false);
        assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::Down)));
        handle_key(VK_CONTROL, false, false, false);
        assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::Up)));

        set_target(vec![VK_CONTROL, VK_SHIFT]);
        handle_key(VK_CONTROL, true, false, false);
        assert!(rx.try_recv().is_err());
        handle_key(VK_SHIFT, true, false, false);
        assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::Down)));
        handle_key(VK_SHIFT, false, false, false);
        assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::Up)));
        handle_key(VK_CONTROL, false, false, false);

        set_capturing(true);
        assert!(!handle_key(VK_CONTROL, true, false, false));
        assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::CaptureProgress(keys)) if keys == vec![VK_CONTROL]));
        assert!(!handle_key(VK_CONTROL, false, false, false));
        assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::Captured(keys)) if keys == vec![VK_CONTROL]));

        set_capturing(true);
        assert!(handle_key(VK_LWIN, true, false, false));
        assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::CaptureProgress(keys)) if keys == vec![VK_LWIN]));
        assert!(handle_key(VK_LWIN, false, false, false));
        assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::Captured(keys)) if keys == vec![VK_LWIN]));

        // A webview capture can end before the low-level hook sees key-up.
        // The next global key-down must still start a fresh shortcut.
        set_target(vec![VK_CONTROL]);
        set_capturing(true);
        handle_key(VK_CONTROL, true, false, false);
        assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::CaptureProgress(_))));
        set_capturing(false);
        handle_key(VK_CONTROL, true, false, false);
        assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::Down)));
        handle_key(VK_CONTROL, false, false, false);
        assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::Up)));
        assert_eq!(GESTURE_OWNER.load(Ordering::SeqCst), OWNER_HOOK);
        release_gesture();

        // While Yappers's window is focused the hook tracks keys silently...
        set_target(vec![VK_CONTROL, VK_LWIN]);
        handle_key(VK_CONTROL, true, false, true);
        handle_key(VK_LWIN, true, false, true);
        assert!(rx.try_recv().is_err());
        assert_eq!(GESTURE_OWNER.load(Ordering::SeqCst), OWNER_NONE);
        // ...but still knows the combo is held if focus moves to another app.
        handle_key(VK_LWIN, false, false, false);
        assert!(matches!(rx.try_recv(), Ok(HotkeyEvent::Up)));
        handle_key(VK_CONTROL, false, false, false);
    }
}
