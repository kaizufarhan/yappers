# Yappers

Free, private voice dictation for Windows. Hold a shortcut, speak, and your words are typed into whatever app you're using. Speech is transcribed entirely on your computer: no account, no subscription, and your audio never leaves your machine.

## Install

1. Download the latest `Yappers_x.y.z_x64-setup.exe` from the [Releases](../../releases) page and run it.
2. Windows may say **"Windows protected your PC"**, because the app isn't code-signed yet. Click **More info → Run anyway**.
3. On first launch, Yappers downloads its speech model (about 640 MB, one time). After that it works fully offline.

Yappers starts with Windows and lives in the system tray. Closing the window keeps it running; quit from the tray icon.

**Requirements:** Windows 10 or 11 (64-bit), a microphone, about 1 GB of free disk space, and 8 GB of RAM recommended. No GPU needed.

## How to use

| Gesture | What happens |
|---|---|
| **Hold** `Ctrl` + `Win` | Records while held; release to paste the text |
| **Double-tap** `Ctrl` + `Win` | Hands-free mode; press `Ctrl` + `Win` once more (or click ✓) to finish |
| `Esc` | Cancels the current recording |

- **Live preview:** the bubble at the bottom of the screen shows your words as you speak. The text that gets pasted comes from one full, more accurate pass when you finish.
- **History:** every dictation is saved in the Yappers window and left on your clipboard, so nothing is lost if a paste doesn't land.
- **Settings:** change the shortcut, pick a microphone, edit which filler words are removed ("um", "uh", or your own like "you know"), choose start/stop sounds, and more.

## Privacy

Everything stays on your computer. The only network request Yappers makes is the one-time model download from Hugging Face. There is no telemetry and no account.

## How it works

- **Speech-to-text:** [NVIDIA Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) (int8 [ONNX export](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx), 25 languages) via [transcribe-rs](https://github.com/cjpais/transcribe-rs) and ONNX Runtime. It runs on the CPU at roughly 10× real time on a typical desktop.
- **Cleanup:** a rule-based pass removes filler words and stutters ("I I think"). There is no AI rewriting.
- **Insertion:** the text is put on the clipboard and Yappers sends `Ctrl+V` to the app you were using.
- **Stack:** [Tauri 2](https://tauri.app) (Rust) with a React + Tailwind UI.

```
src-tauri/src/
  lib.rs         app setup, tray
  hotkey.rs      low-level keyboard hook (modifier-only combos, hold vs double-tap)
  dictation.rs   gesture state machine: record → transcribe → paste; live preview
  audio.rs       microphone capture, resampling to 16 kHz
  model.rs       model download (resumable) and inference
  cleanup.rs     filler-word and stutter removal
  paste.rs       clipboard + simulated Ctrl+V
  overlay.rs     the floating bubble (never takes focus)
  history.rs     local SQLite history
  settings.rs    settings file
  sound.rs       start/stop sound cues
  commands.rs    functions the UI calls
src/             main window (React)
src/overlay/     the dictation bubble (React)
```

## Development

Prerequisites: [Rust](https://rustup.rs), Node.js 20+, and the Visual Studio C++ Build Tools.

```bash
npm install
npm run tauri dev
```

In PowerShell, if scripts are blocked, use `npm.cmd run tauri dev`.

Build the installer (outputs to `src-tauri/target/release/bundle/nsis/`):

```bash
npm run tauri build
```

Other tools:

- **Design previews without the app:** with the dev server running, open `http://localhost:1420/dev/app-preview.html` (main window with sample data) or `/dev/overlay-preview.html` (the bubble, with a replayed dictation).
- **Benchmark transcription** on a 16 kHz mono 16-bit WAV file:
  ```bash
  cd src-tauri
  cargo run --release --example transcribe_file -- "%APPDATA%/app.murmur.desktop/models/parakeet-tdt-0.6b-v3-int8" file.wav
  ```
- **Regenerate the sound cues:** `python tools/generate_sounds.py`
- **Regenerate the app icons** after editing `src-tauri/icons/yappers.svg`: `npx tauri icon src-tauri/icons/yappers.svg`

## Data locations

Settings, history and the speech model live in `%APPDATA%\app.murmur.desktop\` (`settings.json`, `history.db`, `models\`). Logs are in `%LOCALAPPDATA%\app.murmur.desktop\logs\`. The folder keeps the project's original internal ID, so renaming the app didn't orphan existing data.

## Known limitations

- Windows only for now; macOS is planned.
- Windows blocks pasting into apps running as administrator unless Yappers also runs as administrator.
- Unusual names and acronyms can be misheard.

## Credits

- [NVIDIA Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) (CC BY 4.0), ONNX export by [istupakov](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx)
- [transcribe-rs](https://github.com/cjpais/transcribe-rs), [ONNX Runtime](https://onnxruntime.ai), [Tauri](https://tauri.app)
- Fonts: [Archivo](https://fonts.google.com/specimen/Archivo), [DM Mono](https://fonts.google.com/specimen/DM+Mono) and [Figtree](https://fonts.google.com/specimen/Figtree) (SIL Open Font License)
- Icons: [Lucide](https://lucide.dev) (ISC). Animation: [Motion](https://motion.dev) (MIT)

## License

[MIT](LICENSE)
