import { useEffect, useRef, useState } from "react";
import { FolderOpen, Keyboard, Mic, Play, Plus, RotateCcw, Trash2, Volume2, X } from "lucide-react";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { api, on, type Settings } from "../lib/api";
import { Button, Card, Keys, Label, Select, Toggle } from "../components/ui";

const DEFAULT_HOTKEY = [0x11, 0x5b]; // Ctrl + Win

export function SettingsPage({
  settings,
  hotkeyLabels,
  onSave,
  onShortcutCaptureChange,
  focusShortcut,
}: {
  settings: Settings;
  hotkeyLabels: string[];
  onSave: (s: Settings) => Promise<void>;
  onShortcutCaptureChange: (capturing: boolean) => void;
  /** Bumped by the Home "Change key" button to bring the shortcut card into view. */
  focusShortcut: number;
}) {
  const [mics, setMics] = useState<string[]>([]);
  const [launchAtLogin, setLaunchAtLogin] = useState(false);
  const [confirmClear, setConfirmClear] = useState(false);

  useEffect(() => {
    api.listMicrophones().then(setMics);
    isEnabled().then(setLaunchAtLogin).catch(() => {});
  }, []);

  const set = <K extends keyof Settings>(key: K, value: Settings[K]) => onSave({ ...settings, [key]: value });

  const toggleLaunch = async (on: boolean) => {
    await (on ? enable() : disable());
    setLaunchAtLogin(await isEnabled());
  };

  const clearHistory = async () => {
    if (!confirmClear) {
      setConfirmClear(true);
      setTimeout(() => setConfirmClear(false), 3000);
      return;
    }
    await api.clearHistory();
    setConfirmClear(false);
  };

  return (
    <div className="grid grid-cols-2 gap-5">
      <ShortcutCard
        labels={hotkeyLabels}
        isDefault={settings.hotkey.join() === DEFAULT_HOTKEY.join()}
        onCaptured={(keys) => set("hotkey", keys)}
        onReset={() => set("hotkey", DEFAULT_HOTKEY)}
        onCapturingChange={onShortcutCaptureChange}
        focus={focusShortcut}
      />

      <Card title="Microphone">
        <Select
          value={settings.microphone ?? ""}
          onChange={(v) => set("microphone", v || null)}
          icon={<Mic size={15} />}
        >
          <option value="">System default</option>
          {mics.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </Select>
        <p className="mt-3 text-[12.5px] text-muted">
          Yappers only opens the microphone while you're dictating.
        </p>
      </Card>

      <Card title="Text" className="col-span-2">
        <div className="divide-y divide-ink/25">
          <div>
            <Toggle
              label="Remove filler words"
              description={'Strips the words below and repeated words like "I I think".'}
              checked={settings.removeFillers}
              onChange={(v) => set("removeFillers", v)}
            />
            {settings.removeFillers && (
              <FillerWords words={settings.fillerWords} onChange={(words) => set("fillerWords", words)} />
            )}
          </div>
          <Toggle
            label="Paste into the active app"
            description="When off, dictations are only copied to your clipboard and saved to History."
            checked={settings.pasteIntoApp}
            onChange={(v) => set("pasteIntoApp", v)}
          />
          <Toggle
            label="Add a space after each dictation"
            description="Keeps back-to-back dictations from running together."
            checked={settings.trailingSpace}
            onChange={(v) => set("trailingSpace", v)}
          />
        </div>
      </Card>

      <Card title="System">
        <div className="divide-y divide-ink/25">
          <Toggle
            label="Launch at login"
            description="Starts quietly in the system tray."
            checked={launchAtLogin}
            onChange={toggleLaunch}
          />
          <Toggle
            label="Dictation sounds"
            description="Play a short cue when recording starts and ends."
            checked={settings.soundCues}
            onChange={(v) => set("soundCues", v)}
          />
          <div className="py-3">
            <label htmlFor="sound-style" className="block text-[14px] font-semibold text-ink">Sound style</label>
            <p className="mt-0.5 text-[12.5px] text-muted">Choose a softer cue for starting and stopping.</p>
            <div className="mt-3 flex gap-2">
              <Select
                id="sound-style"
                value={settings.soundStyle}
                onChange={(v) => set("soundStyle", v as Settings["soundStyle"])}
                icon={<Volume2 size={15} />}
                className="min-w-0 flex-1"
              >
                <option value="airy">Airy pop</option>
                <option value="glass">Glass shimmer</option>
                <option value="whoosh">Soft whoosh</option>
              </Select>
              <Button className="h-10" icon={<Play size={13} />} onClick={() => api.previewSound(settings.soundStyle)}>
                Preview
              </Button>
            </div>
          </div>
        </div>
      </Card>

      <Card title="Data">
        <p className="text-[12.5px] text-muted">History, settings and the speech model are stored locally.</p>
        <div className="mt-4 flex gap-2">
          <Button icon={<FolderOpen size={15} />} onClick={() => api.openDataFolder()}>
            Open folder
          </Button>
          <Button variant="danger" icon={<Trash2 size={15} />} onClick={clearHistory}>
            {confirmClear ? "Click again to confirm" : "Clear history"}
          </Button>
        </div>
      </Card>
    </div>
  );
}

const normalize = (word: string) => word.trim().replace(/\s+/g, " ").toLowerCase();

/**
 * Editable list of words and phrases to strip from dictation, shown as
 * removable tags with an input to add more.
 */
function FillerWords({ words, onChange }: { words: string[]; onChange: (words: string[]) => void }) {
  const [draft, setDraft] = useState("");
  const [defaults, setDefaults] = useState<string[] | null>(null);

  useEffect(() => {
    api.defaultFillerWords().then(setDefaults).catch(() => {});
  }, []);

  const add = (e: React.FormEvent) => {
    e.preventDefault();
    const word = normalize(draft);
    if (word && !words.includes(word)) onChange([...words, word]);
    setDraft("");
  };

  const isDefault = defaults !== null && defaults.join("|") === words.join("|");

  return (
    <div className="mb-4 border-2 border-ink bg-canvas p-4">
      <div className="flex items-center justify-between gap-4">
        <Label htmlFor="filler-input">Filler words</Label>
        {!isDefault && defaults && (
          <Button icon={<RotateCcw size={13} />} onClick={() => onChange(defaults)} className="h-8 px-3 text-[11px]">
            Reset to defaults
          </Button>
        )}
      </div>

      <ul className="mt-3 flex flex-wrap gap-2" aria-label="Filler words">
        {words.length === 0 && <li className="text-[12.5px] text-muted">No filler words. Add some below.</li>}
        {words.map((word) => (
          <li key={word} className="inline-flex items-stretch border-2 border-ink bg-card">
            <span className="px-2 py-1 font-mono text-[12.5px] font-medium">{word}</span>
            <button
              aria-label={`Remove "${word}"`}
              title="Remove"
              onClick={() => onChange(words.filter((w) => w !== word))}
              className="flex cursor-pointer items-center border-l-2 border-ink px-1.5 transition-colors hover:bg-ink hover:text-on-ink"
            >
              <X size={13} strokeWidth={2.5} />
            </button>
          </li>
        ))}
      </ul>

      <form onSubmit={add} className="mt-3 flex gap-2">
        <input
          id="filler-input"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder='Add a word or phrase, like "basically" or "you know"'
          maxLength={60}
          className="focus-ring h-10 min-w-0 flex-1 border-2 border-ink bg-card px-3 text-[13px] font-medium text-ink placeholder:text-faint"
        />
        <Button type="submit" variant="primary" icon={<Plus size={14} />} disabled={!normalize(draft)}>
          Add
        </Button>
      </form>
      <p className="mt-2.5 text-[12px] text-muted">
        Only removed when they stand alone, so "um" never touches "umbrella". Longer versions like "ummm" are caught
        too.
      </p>
    </div>
  );
}

function ShortcutCard({
  labels,
  isDefault,
  onCaptured,
  onReset,
  onCapturingChange,
  focus,
}: {
  labels: string[];
  isDefault: boolean;
  onCaptured: (keys: number[]) => Promise<void>;
  onReset: () => void;
  onCapturingChange: (capturing: boolean) => void;
  focus: number;
}) {
  const [capturing, setCapturing] = useState(false);
  const [highlight, setHighlight] = useState(false);

  // Arriving from Home's "Change key": scroll here and flash a coral outline.
  useEffect(() => {
    if (focus === 0) return;
    document.getElementById("dictation-shortcut")?.scrollIntoView({ behavior: "smooth", block: "start" });
    setHighlight(true);
    const t = setTimeout(() => setHighlight(false), 1400);
    return () => clearTimeout(t);
  }, [focus]);
  const [pressed, setPressed] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const capturingRef = useRef(false);
  const finishedRef = useRef(false);
  // The global hook sees every key (the window never receives Alt or Win while
  // capturing, since the hook swallows them), so its result wins. The window's
  // own key events are only a fallback for when another program blocks the hook.
  const hookReportedRef = useRef(false);
  const heldKeys = useRef(new Set<number>());
  const chordKeys = useRef(new Set<number>());
  // Keep the latest callback in a ref so the listeners below are registered
  // once; re-subscribing on every render can drop events in between.
  const onCapturedRef = useRef(onCaptured);
  onCapturedRef.current = onCaptured;

  useEffect(() => {
    const finish = async (keys: number[]) => {
      if (!capturingRef.current || finishedRef.current) return;
      finishedRef.current = true;
      capturingRef.current = false;
      onCapturingChange(false);
      heldKeys.current.clear();
      chordKeys.current.clear();
      setCapturing(false);
      setPressed([]);
      await api.cancelHotkeyCapture();
      try {
        await onCapturedRef.current(keys);
      } catch (e) {
        setError(String(e));
      }
    };
    const keyCode = (event: KeyboardEvent) => {
      const vk = event.keyCode;
      return vk === 92 ? 91 : vk;
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (!capturingRef.current) return;
      event.preventDefault();
      event.stopPropagation();
      const vk = keyCode(event);
      if (!vk) return;
      heldKeys.current.add(vk);
      chordKeys.current.add(vk);
      void api.hotkeyLabels([...chordKeys.current]).then((labels) => {
        if (capturingRef.current && !hookReportedRef.current) setPressed(labels);
      });
    };
    const onKeyUp = (event: KeyboardEvent) => {
      if (!capturingRef.current) return;
      event.preventDefault();
      event.stopPropagation();
      heldKeys.current.delete(keyCode(event));
      if (heldKeys.current.size === 0 && chordKeys.current.size > 0 && !hookReportedRef.current) {
        const keys = [...chordKeys.current];
        // Give the hook's result time to arrive before falling back.
        setTimeout(() => {
          if (!hookReportedRef.current) void finish(keys);
        }, 300);
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    const subs = [
      on<{ labels: string[] }>("hotkey-capture-progress", ({ labels }) => {
        if (!capturingRef.current) return;
        hookReportedRef.current = true;
        setPressed(labels);
      }),
      on<{ keys: number[] }>("hotkey-captured", ({ keys }) => void finish(keys)),
    ];
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      subs.forEach((p) => p.then((unlisten) => unlisten()));
      void api.cancelHotkeyCapture();
      onCapturingChange(false);
    };
  }, []);

  const toggle = async () => {
    setPressed([]);
    if (capturing) {
      capturingRef.current = false;
      onCapturingChange(false);
      heldKeys.current.clear();
      chordKeys.current.clear();
      await api.cancelHotkeyCapture();
      setCapturing(false);
    } else {
      setError(null);
      finishedRef.current = false;
      hookReportedRef.current = false;
      heldKeys.current.clear();
      chordKeys.current.clear();
      await api.startHotkeyCapture();
      capturingRef.current = true;
      onCapturingChange(true);
      setCapturing(true);
    }
  };

  return (
    <Card
      id="dictation-shortcut"
      title="Dictation shortcut"
      className={`scroll-mt-8 outline-offset-4 transition-[outline-color] duration-500 ${
        highlight ? "outline-4 outline-coral" : "outline-4 outline-transparent"
      }`}
    >
      <div
        // Inverts to solid black while listening for the new shortcut.
        className={`flex h-16 items-center justify-center border-2 border-ink transition-colors ${
          capturing ? "bg-ink" : "bg-card"
        }`}
      >
        {capturing && pressed.length === 0 && (
          <span className="text-[12px] font-bold tracking-wide text-on-ink uppercase">Press a key or key combination…</span>
        )}
        {capturing && pressed.length > 0 && <Keys labels={pressed} size="lg" />}
        {!capturing && <Keys labels={labels} size="lg" />}
      </div>
      {capturing && (
        <p className="mt-2 text-[12px] text-muted">Hold all keys together, then release to save. Letters and Space need a modifier or F-key.</p>
      )}
      <div className="mt-4 flex items-center gap-2">
        <Button icon={<Keyboard size={15} />} onClick={toggle}>
          {capturing ? "Cancel" : "Change shortcut"}
        </Button>
        {!isDefault && !capturing && (
          <Button icon={<RotateCcw size={14} />} onClick={onReset}>
            Reset
          </Button>
        )}
      </div>
      {error && <p className="mt-3 text-[12.5px] text-danger">{error}</p>}
    </Card>
  );
}
