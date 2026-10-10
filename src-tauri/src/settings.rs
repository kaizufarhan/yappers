use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Virtual-key codes, normalized so left/right variants compare equal.
pub const VK_SHIFT: u32 = 0x10;
pub const VK_CONTROL: u32 = 0x11;
pub const VK_MENU: u32 = 0x12; // Alt
pub const VK_LWIN: u32 = 0x5B;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SoundStyle {
    Airy,
    Glass,
    Whoosh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Keys that must all be held to trigger dictation (normalized VK codes).
    pub hotkey: Vec<u32>,
    /// Input device name; `None` follows the system default.
    pub microphone: Option<String>,
    pub remove_fillers: bool,
    /// Words and phrases stripped when `remove_fillers` is on. User-editable.
    pub filler_words: Vec<String>,
    pub trailing_space: bool,
    pub paste_into_app: bool,
    /// Audible cues when a dictation becomes visible and when it ends.
    pub sound_cues: bool,
    pub sound_style: SoundStyle,
    /// Set once launch-at-login has been enabled on first run, so a user who
    /// later turns it off isn't overridden.
    pub autostart_configured: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: vec![VK_CONTROL, VK_LWIN],
            microphone: None,
            remove_fillers: true,
            filler_words: crate::cleanup::default_fillers(),
            trailing_space: true,
            paste_into_app: true,
            sound_cues: true,
            sound_style: SoundStyle::Airy,
            autostart_configured: false,
        }
    }
}

impl Settings {
    pub fn load(path: &PathBuf) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &PathBuf) -> std::io::Result<()> {
        std::fs::write(path, serde_json::to_string_pretty(self)?)
    }
}

/// Human-readable label for a normalized virtual-key code.
pub fn key_label(vk: u32) -> String {
    match vk {
        VK_CONTROL => "Ctrl".into(),
        VK_MENU => "Alt".into(),
        VK_SHIFT => "Shift".into(),
        VK_LWIN => "Win".into(),
        0x20 => "Space".into(),
        0x1B => "Esc".into(),
        0x09 => "Tab".into(),
        0x14 => "Caps Lock".into(),
        0x13 => "Pause".into(),
        0x2C => "Print Screen".into(),
        0x2D => "Insert".into(),
        0x5D => "Menu".into(),
        0x90 => "Num Lock".into(),
        0x91 => "Scroll Lock".into(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk).unwrap().to_string(),
        _ => format!("Key {vk:#04X}"),
    }
}

/// Order modifiers first so labels read naturally ("Ctrl + Alt", not "Alt + Ctrl").
pub fn sort_keys(keys: &mut [u32]) {
    let rank = |vk: u32| match vk {
        VK_CONTROL => 0,
        VK_LWIN => 1,
        VK_MENU => 2,
        VK_SHIFT => 3,
        _ => 4,
    };
    keys.sort_by_key(|&k| (rank(k), k));
}

#[cfg(test)]
mod tests {
    use super::{Settings, SoundStyle};

    #[test]
    fn old_settings_receive_enabled_sound_cues() {
        let settings: Settings = serde_json::from_str(r#"{"hotkey":[17],"playSounds":false}"#).unwrap();
        assert!(settings.sound_cues);
        assert!(matches!(settings.sound_style, SoundStyle::Airy));
    }

    #[test]
    fn old_settings_receive_default_filler_words() {
        let settings: Settings = serde_json::from_str(r#"{"hotkey":[17],"removeFillers":true}"#).unwrap();
        assert_eq!(settings.filler_words, crate::cleanup::default_fillers());
    }
}
