use crate::dictation::Command;
use crate::hotkey::HotkeyEvent;
use crate::history::{Entry, Stats};
use crate::model::ModelStatus;
use crate::settings::{key_label, sort_keys, Settings, SoundStyle, VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT};
use crate::{audio, cleanup, hotkey, paste, sound, Core};
use std::sync::Arc;
use tauri::{Manager, State};
use tauri_plugin_opener::OpenerExt;

type CoreState<'a> = State<'a, Arc<Core>>;

#[tauri::command]
pub fn get_settings(core: CoreState) -> Settings {
    core.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn update_settings(core: CoreState, mut settings: Settings) -> Result<Settings, String> {
    validate_hotkey(&settings.hotkey)?;
    sort_keys(&mut settings.hotkey);
    settings.filler_words = sanitize_fillers(&settings.filler_words);
    settings.save(&core.settings_path).map_err(|e| e.to_string())?;
    hotkey::set_target(settings.hotkey.clone());
    *core.settings.lock().unwrap() = settings.clone();
    Ok(settings)
}

/// Normalizes the user's filler list: tidy spacing and case, drop blanks and
/// duplicates, and cap its size so a pasted wall of text can't bloat settings.
fn sanitize_fillers(words: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in words {
        let word = cleanup::normalize_filler(word);
        if !word.is_empty() && word.chars().count() <= 60 && !out.contains(&word) {
            out.push(word);
        }
    }
    out.truncate(200);
    out
}

#[tauri::command]
pub fn default_filler_words() -> Vec<String> {
    cleanup::default_fillers()
}

fn validate_hotkey(keys: &[u32]) -> Result<(), String> {
    // A bare printable key would start dictation during normal typing. Allow
    // nonprinting single keys and combinations that contain a modifier/F-key.
    let is_modifier = |k: &u32| matches!(*k, VK_CONTROL | VK_MENU | VK_SHIFT | VK_LWIN);
    let is_fn_key = |k: &u32| (0x70..=0x87).contains(k);
    let is_special = |k: &u32| matches!(*k, 0x13 | 0x14 | 0x2C | 0x2D | 0x5D | 0x90 | 0x91);
    if keys.is_empty() ||
        (keys.len() == 1 && !keys.iter().any(|k| is_modifier(k) || is_fn_key(k) || is_special(k))) ||
        (keys.len() > 1 && !keys.iter().any(|k| is_modifier(k) || is_fn_key(k))) {
        return Err("Use Ctrl, Alt, Shift, Win, an F-key, or a special key by itself; pair letters and Space with a modifier".into());
    }
    Ok(())
}

#[tauri::command]
pub fn list_microphones() -> Vec<String> {
    audio::list_input_devices()
}

#[tauri::command]
pub fn get_history(core: CoreState, query: Option<String>, limit: Option<i64>) -> Result<Vec<Entry>, String> {
    core.history.lock().unwrap().list(query.as_deref(), limit.unwrap_or(200)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_history_entry(core: CoreState, id: i64) -> Result<(), String> {
    core.history.lock().unwrap().delete(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_history(core: CoreState) -> Result<(), String> {
    core.history.lock().unwrap().clear().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_stats(core: CoreState) -> Result<Stats, String> {
    core.history.lock().unwrap().stats().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn copy_text(text: String) -> Result<(), String> {
    paste::copy_to_clipboard(&text)
}

#[tauri::command]
pub fn model_status(core: CoreState) -> ModelStatus {
    core.model.status()
}

#[tauri::command]
pub async fn download_model(core: CoreState<'_>) -> Result<(), String> {
    let core = core.inner().clone();
    core.model.download(&core.app).await?;
    std::thread::spawn(move || core.model.load(&core.app));
    Ok(())
}

#[tauri::command]
pub fn start_hotkey_capture() {
    hotkey::set_capturing(true);
}

#[tauri::command]
pub fn cancel_hotkey_capture() {
    hotkey::set_capturing(false);
}

#[tauri::command]
pub fn local_hotkey(core: CoreState, event: String) {
    if hotkey::is_capturing() {
        return;
    }
    // A gesture that started in another app belongs to the global hook; the
    // window must not feed it a second copy of the same key events.
    use std::sync::atomic::Ordering;
    let owner = &hotkey::GESTURE_OWNER;
    if owner.load(Ordering::SeqCst) == hotkey::OWNER_HOOK {
        return;
    }
    if event == "down" {
        let _ = owner.compare_exchange(hotkey::OWNER_NONE, hotkey::OWNER_WEBVIEW, Ordering::SeqCst, Ordering::SeqCst);
    }
    let event = match event.as_str() {
        "down" => HotkeyEvent::Down,
        "up" => HotkeyEvent::Up,
        "interrupted" => HotkeyEvent::Interrupted,
        "escape" => HotkeyEvent::Escape,
        _ => return,
    };
    let _ = core.commands.send(Command::Hotkey(event));
}

#[tauri::command]
pub fn hotkey_labels(keys: Vec<u32>) -> Vec<String> {
    keys.into_iter().map(key_label).collect()
}

#[tauri::command]
pub fn overlay_stop(core: CoreState) {
    let _ = core.commands.send(Command::Stop);
}

#[tauri::command]
pub fn overlay_cancel(core: CoreState) {
    let _ = core.commands.send(Command::Cancel);
}

#[tauri::command]
pub fn overlay_snapshot() -> crate::dictation::OverlaySnapshot {
    crate::dictation::overlay_snapshot()
}

#[tauri::command]
pub fn preview_sound(style: SoundStyle) {
    sound::preview(style);
}

#[tauri::command]
pub fn open_data_folder(core: CoreState) -> Result<(), String> {
    let dir = core.app.path().app_data_dir().map_err(|e| e.to_string())?;
    core.app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{sanitize_fillers, validate_hotkey};

    #[test]
    fn sanitizes_filler_list() {
        let input: Vec<String> = ["  Um ", "um", "", "You   Know"].iter().map(|s| s.to_string()).collect();
        assert_eq!(sanitize_fillers(&input), vec!["um".to_string(), "you know".to_string()]);
    }

    #[test]
    fn accepts_single_special_keys_and_modified_chords() {
        assert!(validate_hotkey(&[0x11]).is_ok()); // Ctrl
        assert!(validate_hotkey(&[0x77]).is_ok()); // F8
        assert!(validate_hotkey(&[0x14]).is_ok()); // Caps Lock
        assert!(validate_hotkey(&[0x11, 0x4B]).is_ok()); // Ctrl+K
        assert!(validate_hotkey(&[0x11, 0x20]).is_ok()); // Ctrl+Space
    }

    #[test]
    fn rejects_shortcuts_that_hijack_typing() {
        assert!(validate_hotkey(&[]).is_err());
        assert!(validate_hotkey(&[0x41]).is_err()); // A
        assert!(validate_hotkey(&[0x20]).is_err()); // Space
        assert!(validate_hotkey(&[0x41, 0x42]).is_err()); // A+B
    }
}
