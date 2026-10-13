mod audio;
mod cleanup;
mod commands;
mod dictation;
mod history;
mod hotkey;
mod model;
mod overlay;
mod paste;
mod settings;
mod sound;
mod updater;

use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};

/// Shared application state.
pub struct Core {
    pub app: AppHandle,
    pub settings: Mutex<settings::Settings>,
    pub settings_path: PathBuf,
    pub history: Mutex<history::History>,
    pub model: model::Model,
    pub commands: mpsc::Sender<dictation::Command>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .level_for("transcribe_rs", log::LevelFilter::Warn)
                .level_for("ort", log::LevelFilter::Warn)
                .max_file_size(2_000_000)
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(data_dir.join("models"))?;

            let settings_path = data_dir.join("settings.json");
            let settings = settings::Settings::load(&settings_path);
            hotkey::set_target(settings.hotkey.clone());
            if let Some(main) = app.get_webview_window("main") {
                hotkey::set_main_window(main.hwnd()?.0 as isize);
                overlay::square_corners(main.hwnd()?.0 as isize);
            }

            let (cmd_tx, cmd_rx) = mpsc::channel();
            let core = Arc::new(Core {
                app: handle.clone(),
                settings: Mutex::new(settings),
                settings_path,
                history: Mutex::new(history::History::open(&data_dir.join("history.db"))?),
                model: model::Model::new(&data_dir.join("models")),
                commands: cmd_tx.clone(),
            });
            app.manage(core.clone());

            // Create the overlay before keyboard input can start a dictation.
            overlay::create(&handle)?;

            // Keyboard hook → dictation state machine.
            let (key_tx, key_rx) = mpsc::channel();
            hotkey::start(key_tx);
            std::thread::spawn(move || {
                for event in key_rx {
                    if cmd_tx.send(dictation::Command::Hotkey(event)).is_err() {
                        break;
                    }
                }
            });
            dictation::spawn(core.clone(), cmd_rx);

            setup_tray(&handle)?;

            if core.model.is_downloaded() {
                let core = core.clone();
                std::thread::spawn(move || core.model.load(&core.app));
            }
            updater::spawn(handle.clone());

            // Launch-at-login is only managed by installed builds: a dev build
            // can't run without the dev server, so it must never register.
            #[cfg(not(debug_assertions))]
            {
                use tauri_plugin_autostart::ManagerExt;
                let autolaunch = app.autolaunch();
                let mut settings = core.settings.lock().unwrap();
                // Turn it on the first time an installed build runs, and
                // otherwise re-register while it's on, so the login entry
                // always points at this installed copy.
                if !settings.autostart_configured || autolaunch.is_enabled().unwrap_or(false) {
                    if let Err(e) = autolaunch.enable() {
                        log::warn!("couldn't enable launch at login: {e}");
                    }
                }
                if !settings.autostart_configured {
                    settings.autostart_configured = true;
                    let _ = settings.save(&core.settings_path);
                }
                remove_legacy_login_entry();
            }

            // Launched at login: stay in the tray unless setup is unfinished.
            let hidden = std::env::args().any(|a| a == "--hidden");
            if !hidden || !core.model.is_downloaded() {
                dictation::show_main_window(&core);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window keeps Yappers running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::update_settings,
            commands::list_microphones,
            commands::get_history,
            commands::delete_history_entry,
            commands::clear_history,
            commands::get_stats,
            commands::copy_text,
            commands::model_status,
            commands::download_model,
            commands::start_hotkey_capture,
            commands::cancel_hotkey_capture,
            commands::local_hotkey,
            commands::hotkey_labels,
            commands::overlay_stop,
            commands::overlay_cancel,
            commands::overlay_snapshot,
            commands::preview_sound,
            commands::default_filler_words,
            commands::open_data_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Yappers");
}

/// Removes the login entry left by builds from when the app was called
/// Murmur. If it pointed at an old or dev build, it could start first at
/// login, and only one copy of the app may run, so the real one would quit.
#[cfg(not(debug_assertions))]
fn remove_legacy_login_entry() {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegDeleteKeyValueW, HKEY_CURRENT_USER};
    unsafe {
        let _ = RegDeleteKeyValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Run"),
            w!("Murmur"),
        );
    }
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Yappers", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Yappers", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Yappers")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
