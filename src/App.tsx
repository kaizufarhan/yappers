import { useCallback, useEffect, useState } from "react";
import { CircleCheck, Download, House, LoaderCircle, Settings as SettingsIcon, TriangleAlert } from "lucide-react";
import { api, on, type ModelStatus, type Settings } from "./lib/api";
import { Home } from "./pages/Home";
import { SettingsPage } from "./pages/Settings";
import { TitleBar } from "./components/TitleBar";

type Page = "home" | "settings";

export default function App() {
  const [page, setPage] = useState<Page>("home");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [hotkeyLabels, setHotkeyLabels] = useState<string[]>([]);
  const [model, setModel] = useState<ModelStatus>({ state: "loading" });
  const [capturingShortcut, setCapturingShortcut] = useState(false);
  const [shortcutFocus, setShortcutFocus] = useState(0);

  const openShortcutSettings = () => {
    setPage("settings");
    setShortcutFocus((n) => n + 1);
  };

  const applySettings = useCallback(async (s: Settings) => {
    setSettings(s);
    setHotkeyLabels(await api.hotkeyLabels(s.hotkey));
  }, []);

  useEffect(() => {
    api.getSettings().then(applySettings);
    api.modelStatus().then(setModel);
    const sub = on<ModelStatus>("model-status", setModel);
    return () => void sub.then((unlisten) => unlisten());
  }, [applySettings]);

  const saveSettings = async (next: Settings) => {
    await applySettings(await api.updateSettings(next));
  };

  useEffect(() => {
    if (!settings || capturingShortcut) return;
    const target = settings.hotkey;
    const held = new Set<number>();
    let active = false;
    let queue = Promise.resolve();
    const send = (event: "down" | "up" | "interrupted" | "escape") => {
      queue = queue.then(() => api.localHotkey(event)).catch(() => {});
    };
    const code = (event: KeyboardEvent) => event.keyCode === 92 ? 91 : event.keyCode;
    const down = (event: KeyboardEvent) => {
      const vk = code(event);
      if (!vk || held.has(vk)) return;
      held.add(vk);
      if (vk === 27) {
        // Esc cancels any dictation, including hands-free ones whose shortcut
        // is no longer held. The backend ignores it when nothing is recording.
        active = false;
        send("escape");
      } else if (active && !target.includes(vk)) {
        active = false;
        send("interrupted");
      } else if (!active && held.size === target.length && target.every((key) => held.has(key))) {
        active = true;
        send("down");
      }
    };
    const up = (event: KeyboardEvent) => {
      const vk = code(event);
      held.delete(vk);
      if (active && target.includes(vk)) {
        active = false;
        send("up");
      }
    };
    const blur = () => {
      if (active) send("interrupted");
      active = false;
      held.clear();
    };
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    window.addEventListener("blur", blur);
    return () => {
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("keyup", up, true);
      window.removeEventListener("blur", blur);
    };
  }, [settings?.hotkey, capturingShortcut]);

  if (!settings) return null;

  return (
    <div className="flex h-full flex-col">
      <TitleBar>
        <ModelBadge status={model} />
      </TitleBar>

      <main className="bg-wave min-h-0 flex-1 overflow-y-auto px-8 pt-8 pb-28">
        <div className="mx-auto max-w-5xl">
          {page === "home" ? (
            <Home model={model} hotkeyLabels={hotkeyLabels} onChangeShortcut={openShortcutSettings} />
          ) : (
            <SettingsPage
              settings={settings}
              hotkeyLabels={hotkeyLabels}
              onSave={saveSettings}
              onShortcutCaptureChange={setCapturingShortcut}
              focusShortcut={shortcutFocus}
            />
          )}
        </div>
      </main>

      <BottomNav
        page={page}
        onChange={(p) => {
          setPage(p);
          setShortcutFocus(0); // plain navigation shouldn't scroll/flash the shortcut card
        }}
      />
    </div>
  );
}

/** Plain-language model status, set as a small uppercase label in the title bar. */
function ModelBadge({ status }: { status: ModelStatus }) {
  const pct =
    status.state === "downloading" && status.total > 0 ? Math.round((status.downloaded / status.total) * 100) : null;
  const [Icon, label, iconClass] = {
    ready: [CircleCheck, "Ready to dictate", ""],
    loading: [LoaderCircle, "Getting the speech model ready…", "motion-safe:animate-spin"],
    downloading: [LoaderCircle, pct === null ? "Downloading the speech model…" : `Downloading the speech model · ${pct}%`, "motion-safe:animate-spin"],
    notDownloaded: [Download, "Speech model not downloaded yet", ""],
    error: [TriangleAlert, "Couldn't load the speech model", "text-danger"],
  }[status.state] as [typeof CircleCheck, string, string];
  return (
    <span className="flex items-center gap-1.5 text-[11px] font-bold tracking-wide text-ink uppercase" role="status">
      <Icon size={14} strokeWidth={2.25} className={iconClass} aria-hidden="true" />
      {label}
    </span>
  );
}

/** Floating segmented nav: a bordered box whose segments invert when active. */
function BottomNav({ page, onChange }: { page: Page; onChange: (p: Page) => void }) {
  const items: { id: Page; label: string; icon: typeof House }[] = [
    { id: "home", label: "Home", icon: House },
    { id: "settings", label: "Settings", icon: SettingsIcon },
  ];
  return (
    <nav className="fixed bottom-6 left-1/2 flex -translate-x-1/2 border-2 border-ink bg-card">
      {items.map(({ id, label, icon: Icon }) => (
        <button
          key={id}
          onClick={() => onChange(id)}
          className={`flex cursor-pointer items-center gap-2 border-l-2 border-ink px-5 py-2.5 text-[12px] font-bold tracking-wide uppercase transition-colors first:border-l-0 ${
            page === id ? "bg-ink text-on-ink" : "text-ink hover:bg-ink hover:text-on-ink"
          }`}
        >
          <Icon size={15} />
          {label}
        </button>
      ))}
    </nav>
  );
}
