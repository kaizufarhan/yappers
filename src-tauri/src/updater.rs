//! Automatic updates from GitHub Releases.
//!
//! Installed builds check the latest release shortly after starting and then
//! every few hours. A newer version is downloaded in the background, verified
//! against the public key in tauri.conf.json (so only builds signed by this
//! repo's release workflow are accepted), and installed once no dictation is
//! in progress; the app then restarts into the new version.

use crate::hotkey::RECORDING;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

const FIRST_CHECK_AFTER: Duration = Duration::from_secs(20);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

pub fn spawn(app: AppHandle) {
    // Dev builds run from source; there's nothing for them to update to.
    if cfg!(debug_assertions) {
        return;
    }
    std::thread::Builder::new()
        .name("yappers-updater".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK_AFTER);
            loop {
                if let Err(e) = tauri::async_runtime::block_on(check_and_install(&app)) {
                    // Offline, GitHub unreachable, etc.: try again next time.
                    log::warn!("update check failed: {e}");
                }
                std::thread::sleep(CHECK_EVERY);
            }
        })
        .expect("spawn updater thread");
}

async fn check_and_install(app: &AppHandle) -> tauri_plugin_updater::Result<()> {
    let Some(update) = app.updater()?.check().await? else {
        return Ok(());
    };
    log::info!("downloading update {} (running {})", update.version, update.current_version);
    let bytes = update.download(|_, _| {}, || {}).await?;

    // Installing restarts the app, so never do it mid-dictation.
    while RECORDING.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_secs(2));
    }
    log::info!("installing update {}", update.version);
    update.install(bytes)?;
    app.restart();
}
