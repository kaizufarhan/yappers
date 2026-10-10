//! Inserting text into whatever app has focus: put it on the clipboard and
//! simulate Ctrl+V. This is what most dictation tools do — typing characters
//! one by one is slow and breaks in many apps.

use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SetForegroundWindow,
};

pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text.to_string()))
        .map_err(|e| format!("Clipboard error: {e}"))
}

/// Handle of the window that currently has focus, stored as an integer so it
/// can cross threads.
pub fn foreground_window() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

fn is_own_window(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid == std::process::id()
}

/// Copies `text` and pastes it into the focused app. If Yappers itself grabbed
/// focus during recording (e.g. the overlay was clicked), focus is handed back
/// to `target` first.
pub fn paste(text: &str, target: isize) -> Result<(), String> {
    copy_to_clipboard(text)?;
    wait_for_modifiers_released(Duration::from_millis(1500));

    unsafe {
        let current = GetForegroundWindow();
        let target = HWND(target as *mut _);
        if is_own_window(current) && !target.0.is_null() && IsWindow(Some(target)).as_bool() && !is_own_window(target) {
            let _ = SetForegroundWindow(target);
            std::thread::sleep(Duration::from_millis(60));
        }
    }

    // Give the clipboard a moment to settle before the target app reads it.
    std::thread::sleep(Duration::from_millis(30));
    let key = |vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, dwFlags: flags, dwExtraInfo: crate::hotkey::INPUT_TAG, ..Default::default() },
        },
    };
    let inputs = [
        key(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        key(VK_V, KEYBD_EVENT_FLAGS(0)),
        key(VK_V, KEYEVENTF_KEYUP),
        key(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err("Windows blocked the paste (the target app may be running as administrator)".into());
    }
    Ok(())
}

/// Pasting while the user still physically holds Ctrl/Alt would turn our
/// Ctrl+V into e.g. Ctrl+Alt+V, so wait until the hotkey is fully released.
fn wait_for_modifiers_released(timeout: Duration) {
    let start = Instant::now();
    let held = || {
        [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
            .iter()
            .any(|vk| unsafe { GetAsyncKeyState(vk.0 as i32) } as u16 & 0x8000 != 0)
    };
    while held() && start.elapsed() < timeout {
        std::thread::sleep(Duration::from_millis(15));
    }
}
