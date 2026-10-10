import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Settings {
  hotkey: number[];
  microphone: string | null;
  removeFillers: boolean;
  fillerWords: string[];
  trailingSpace: boolean;
  pasteIntoApp: boolean;
  soundCues: boolean;
  soundStyle: "airy" | "glass" | "whoosh";
  autostartConfigured: boolean;
}

export interface Entry {
  id: number;
  text: string;
  rawText: string;
  createdAt: number;
  durationMs: number;
  wordCount: number;
  pasted: boolean;
}

export interface Stats {
  totalWords: number;
  totalEntries: number;
  totalDurationMs: number;
}

export type ModelStatus =
  | { state: "notDownloaded" }
  | { state: "downloading"; downloaded: number; total: number }
  | { state: "loading" }
  | { state: "ready" }
  | { state: "error"; message: string };

export type OverlayState =
  | { state: "recording" }
  | { state: "locked" }
  | { state: "processing" }
  | { state: "error"; message: string }
  | { state: "hidden" };

export interface OverlaySnapshot {
  revision: number;
  state: OverlayState;
}

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (settings: Settings) => invoke<Settings>("update_settings", { settings }),
  listMicrophones: () => invoke<string[]>("list_microphones"),
  getHistory: (query?: string) => invoke<Entry[]>("get_history", { query: query || null }),
  deleteHistoryEntry: (id: number) => invoke<void>("delete_history_entry", { id }),
  clearHistory: () => invoke<void>("clear_history"),
  getStats: () => invoke<Stats>("get_stats"),
  copyText: (text: string) => invoke<void>("copy_text", { text }),
  modelStatus: () => invoke<ModelStatus>("model_status"),
  downloadModel: () => invoke<void>("download_model"),
  startHotkeyCapture: () => invoke<void>("start_hotkey_capture"),
  cancelHotkeyCapture: () => invoke<void>("cancel_hotkey_capture"),
  localHotkey: (event: "down" | "up" | "interrupted" | "escape") => invoke<void>("local_hotkey", { event }),
  hotkeyLabels: (keys: number[]) => invoke<string[]>("hotkey_labels", { keys }),
  overlayStop: () => invoke<void>("overlay_stop"),
  overlayCancel: () => invoke<void>("overlay_cancel"),
  overlaySnapshot: () => invoke<OverlaySnapshot>("overlay_snapshot"),
  previewSound: (style: Settings["soundStyle"]) => invoke<void>("preview_sound", { style }),
  openDataFolder: () => invoke<void>("open_data_folder"),
  defaultFillerWords: () => invoke<string[]>("default_filler_words"),
};

export function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
}
