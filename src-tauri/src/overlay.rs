//! The floating dictation window. It follows the app receiving dictation and
//! must never take focus, otherwise the paste could land in the overlay.

use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWM_WINDOW_CORNER_PREFERENCE};
use windows::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, SetWindowRgn, HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, IsWindow, IsWindowVisible, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE,
    HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST,
};

pub const LABEL: &str = "overlay";
/// Opening unfolds the bubble from a small square in the middle out to full
/// width; closing folds it back. The bubble is a solid window (see `create`),
/// so instead of fading it the visible area (its window region) is animated.
const UNFOLD: Duration = Duration::from_millis(180);
const FOLD: Duration = Duration::from_millis(120);
const FRAME: Duration = Duration::from_millis(7);
/// Bumped by every show/hide; a running animation stops once it's stale.
static ANIMATION: Mutex<u64> = Mutex::new(0);
const WIDTH: f64 = 300.0;
const HEIGHT: f64 = 48.0;
/// Gap between the overlay and the bottom of the work area (above the taskbar).
const BOTTOM_MARGIN: f64 = 8.0;

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("overlay.html".into()))
        .title("Yappers")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        // Some Windows/WebView2 setups composite transparent windows as fully
        // invisible even while the webview reports a visible, rendered DOM.
        .transparent(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .build()?;
    let hwnd = HWND(window.hwnd()?.0);
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            style | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0) as isize,
        );
    }
    Ok(())
}

/// Opts the main window out of Windows 11's automatic rounded corners. Not
/// used on the overlay: there it made Windows draw a tool-window caption. The
/// overlay is clipped to a plain rectangle instead, which also stops rounding.
pub fn square_corners(hwnd: isize) {
    let preference: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_DONOTROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(
            HWND(hwnd as *mut _),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const _,
            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
    }
}

/// Shows the overlay on the monitor containing the app receiving dictation.
/// The cursor can be parked on another monitor while the user is typing here.
pub fn show(app: &AppHandle, target_window: isize) {
    let Some(window) = app.get_webview_window(LABEL) else { return };
    let Ok(handle) = window.hwnd() else {
        log::warn!("overlay has no native window handle");
        return;
    };
    let hwnd = HWND(handle.0);
    unsafe {
        let target = HWND(target_window as *mut _);
        let foreground = GetForegroundWindow();
        let anchor = if !target.0.is_null() && IsWindow(Some(target)).as_bool() {
            target
        } else {
            foreground
        };
        let monitor = if !anchor.0.is_null() {
            MonitorFromWindow(anchor, MONITOR_DEFAULTTONEAREST)
        } else {
            let mut cursor = POINT::default();
            let _ = GetCursorPos(&mut cursor);
            MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST)
        };
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            log::warn!("couldn't locate monitor for dictation overlay");
            return;
        }
        let work = info.rcWork;
        let scale = window.scale_factor().unwrap_or(1.0);
        let (w, h) = ((WIDTH * scale).round() as i32, (HEIGHT * scale).round() as i32);
        let x = work.left + ((work.right - work.left) - w) / 2;
        let y = work.bottom - h - (BOTTOM_MARGIN * scale) as i32;
        let was_visible = IsWindowVisible(hwnd).as_bool();
        let generation = next_animation();
        // Already open (e.g. recording → pasting): just make sure it's whole.
        // Otherwise start as a small square and unfold.
        if was_visible {
            clip(hwnd, w, w, h);
        } else {
            clip(hwnd, h, w, h);
        }
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        if let Err(e) = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW) {
            log::warn!("couldn't position dictation overlay: {e}");
        }
        if !was_visible {
            animate(hwnd, h, w, w, h, UNFOLD, ease_out_cubic, generation, false);
        }
    }
    // State can change while the WebView is hidden. Resend it after the native
    // window is visible so a missed hidden-window event cannot leave stale UI.
    let _ = app.emit_to(LABEL, "overlay-state", crate::dictation::overlay_snapshot());
}

/// Folds the overlay back to a square and hides it.
pub fn hide(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else { return };
    let hwnd = match window.hwnd() {
        Ok(handle) => HWND(handle.0),
        Err(e) => {
            log::warn!("couldn't get dictation overlay handle to hide: {e}");
            return;
        }
    };
    let generation = next_animation();
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            let _ = ShowWindow(hwnd, SW_HIDE);
            return;
        }
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        animate(hwnd, w, h, w, h, FOLD, ease_in_cubic, generation, true);
    }
}

fn next_animation() -> u64 {
    let mut generation = ANIMATION.lock().unwrap();
    *generation += 1;
    *generation
}

/// Clips the window to a centered strip `visible` pixels wide. Besides the
/// sharp corners, having a window region also keeps Windows from drawing any
/// frame on the overlay.
fn clip(hwnd: HWND, visible: i32, w: i32, h: i32) {
    let left = (w - visible) / 2;
    unsafe {
        let region = CreateRectRgn(left, 0, left + visible, h);
        if SetWindowRgn(hwnd, Some(region), true) == 0 {
            log::warn!("couldn't clip dictation overlay");
            let _ = DeleteObject(HGDIOBJ(region.0));
        }
    }
}

/// Animates the visible width from `from` to `to` on a background thread.
/// Steps are skipped once a newer show/hide has started; `then_hide` hides
/// the window when a fold finishes.
#[allow(clippy::too_many_arguments)]
fn animate(hwnd: HWND, from: i32, to: i32, w: i32, h: i32, duration: Duration, ease: fn(f32) -> f32, generation: u64, then_hide: bool) {
    let hwnd = hwnd.0 as isize; // HWND isn't Send
    std::thread::spawn(move || {
        let hwnd = HWND(hwnd as *mut _);
        let started = Instant::now();
        loop {
            let t = (started.elapsed().as_secs_f32() / duration.as_secs_f32()).min(1.0);
            let visible = from + ((to - from) as f32 * ease(t)).round() as i32;
            {
                let current = ANIMATION.lock().unwrap();
                if *current != generation {
                    return;
                }
                if t >= 1.0 && then_hide {
                    unsafe {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                    clip(hwnd, w, w, h);
                    return;
                }
                clip(hwnd, visible, w, h);
            }
            if t >= 1.0 {
                return;
            }
            std::thread::sleep(FRAME);
        }
    });
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_cubic(t: f32) -> f32 {
    t * t * t
}

