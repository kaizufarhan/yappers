import { useEffect, useMemo, useState, type ReactNode } from "react";
import { Check, Copy, Download, Keyboard, Search, Trash2, TriangleAlert } from "lucide-react";
import { api, on, type Entry, type ModelStatus, type Stats } from "../lib/api";
import { Button, Card, IconButton, Keys, Label } from "../components/ui";
import { dayLabel, formatBytes, formatNumber, formatTime } from "../lib/format";

export function Home({
  model,
  hotkeyLabels,
  onChangeShortcut,
}: {
  model: ModelStatus;
  hotkeyLabels: string[];
  /** Opens Settings at the dictation-shortcut section. */
  onChangeShortcut: () => void;
}) {
  const [entries, setEntries] = useState<Entry[]>([]);
  const [stats, setStats] = useState<Stats | null>(null);
  const [query, setQuery] = useState("");

  const refresh = () => {
    api.getStats().then(setStats);
  };

  useEffect(() => {
    refresh();
    const sub = on<Entry>("history-added", (entry) => {
      setEntries((prev) => [entry, ...prev]);
      refresh();
    });
    return () => void sub.then((unlisten) => unlisten());
  }, []);

  useEffect(() => {
    const t = setTimeout(() => api.getHistory(query).then(setEntries), query ? 150 : 0);
    return () => clearTimeout(t);
  }, [query]);

  const remove = async (id: number) => {
    await api.deleteHistoryEntry(id);
    setEntries((prev) => prev.filter((e) => e.id !== id));
    refresh();
  };

  const needsSetup = model.state === "notDownloaded" || model.state === "downloading" || model.state === "error";

  const wpm = stats && stats.totalDurationMs > 0 ? Math.round(stats.totalWords / (stats.totalDurationMs / 60000)) : null;

  return (
    <div className="grid grid-cols-3 gap-5">
      {needsSetup ? (
        <SetupCard status={model} />
      ) : (
        <Hero>
          <div>
            <div className="flex items-center justify-between gap-4">
              <Label>Dictate anywhere</Label>
              <Button icon={<Keyboard size={14} />} onClick={onChangeShortcut} className="h-8 px-3 text-[11px]">
                Change key
              </Button>
            </div>
            <h1 className="type-display mt-4 text-[40px]">
              <span className="flex flex-wrap items-center gap-x-4 gap-y-2">
                Hold <Keys labels={hotkeyLabels} size="hero" tone="accent" />
              </span>
              <span className="mt-2 block">and speak</span>
            </h1>
          </div>
          <div className="mt-7 grid grid-cols-3 gap-3 text-[13px] font-medium">
            <Tip title="Hold" body="Talk while holding, release to paste." />
            <Tip title="Double-tap" body="Hands-free. Press once more to finish." />
            <Tip title="Esc" body="Cancel without pasting." />
          </div>
        </Hero>
      )}

      <Card className="flex flex-col justify-center !py-1">
        <Stat value={stats ? formatNumber(stats.totalWords) : "–"} label="Words dictated" />
        <Stat value={stats ? formatNumber(stats.totalEntries) : "–"} label="Dictations" />
        <Stat value={wpm === null ? "–" : String(wpm)} label="Words per minute" />
      </Card>

      <Card
        title="History"
        className="col-span-3"
        action={
          <label className="flex h-10 w-64 items-center gap-2 border-2 border-ink bg-card px-3 text-ink focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-ink">
            <Search size={15} />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search dictations…"
              className="w-full bg-transparent text-[13px] font-medium text-ink outline-none placeholder:text-faint"
            />
          </label>
        }
      >
        <HistoryList entries={entries} onDelete={remove} searching={!!query} />
      </Card>
    </div>
  );
}

/** The bordered box at the top of Home. */
function Hero({ children }: { children: ReactNode }) {
  return (
    <section className="rise col-span-2 flex min-w-0 flex-col justify-between border-2 border-ink bg-card p-7">
      {children}
    </section>
  );
}

function Tip({ title, body }: { title: string; body: string }) {
  return (
    <div className="border-2 border-ink px-3 py-2.5">
      <p className="text-[12px] font-bold tracking-wide uppercase">{title}</p>
      <p className="mt-1 leading-snug">{body}</p>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="border-b-2 border-ink py-4 last:border-b-0">
      <p className="type-display text-[40px]">{value}</p>
      <p className="mt-1.5 text-[11px] font-bold tracking-wide text-muted uppercase">{label}</p>
    </div>
  );
}

function SetupCard({ status }: { status: ModelStatus }) {
  const [starting, setStarting] = useState(false);
  const downloading = status.state === "downloading";
  const pct = downloading && status.total > 0 ? (status.downloaded / status.total) * 100 : 0;

  const start = async () => {
    setStarting(true);
    try {
      await api.downloadModel();
    } catch {
      // The error arrives via the model-status event.
    } finally {
      setStarting(false);
    }
  };

  return (
    <Hero>
      <div>
        <Label>One-time setup</Label>
        <h1 className="type-display mt-4 text-[34px]">Download the speech model</h1>
        <p className="mt-3 max-w-md text-[13.5px] leading-relaxed text-muted">
          Yappers uses NVIDIA Parakeet, which runs on any modern CPU. It's about 640 MB and works fully offline once
          downloaded.
        </p>
      </div>

      <div className="mt-7">
        {downloading ? (
          <div>
            <div className="h-5 border-2 border-ink bg-card">
              <div className="h-full bg-ink transition-[width] duration-200" style={{ width: `${pct}%` }} />
            </div>
            <p className="mt-2.5 flex justify-between font-mono text-[12px] font-medium">
              <span>{formatBytes(status.downloaded)} of {formatBytes(status.total)}</span>
              <span>{pct.toFixed(0)}%</span>
            </p>
          </div>
        ) : (
          <div className="flex items-center gap-4">
            <Button variant="primary" icon={<Download size={15} />} onClick={start} disabled={starting}>
              {status.state === "error" ? "Retry download" : "Download model"}
            </Button>
            {status.state === "error" && (
              <span className="flex items-center gap-1.5 text-[12.5px] font-medium text-danger">
                <TriangleAlert size={14} /> {status.message}
              </span>
            )}
          </div>
        )}
      </div>
    </Hero>
  );
}

function HistoryList({
  entries,
  onDelete,
  searching,
}: {
  entries: Entry[];
  onDelete: (id: number) => void;
  searching: boolean;
}) {
  const groups = useMemo(() => {
    const map = new Map<string, Entry[]>();
    for (const e of entries) {
      const key = dayLabel(e.createdAt);
      map.set(key, [...(map.get(key) ?? []), e]);
    }
    return [...map.entries()];
  }, [entries]);

  if (entries.length === 0) {
    return (
      <p className="py-10 text-center text-[13px] text-faint">
        {searching ? "No dictations match your search." : "Your dictations will appear here."}
      </p>
    );
  }

  return (
    <div className="space-y-6">
      {groups.map(([day, items]) => (
        <div key={day}>
          <p className="mb-1 text-[12px] font-bold tracking-wide text-ink uppercase">{day}</p>
          <ul className="divide-y divide-ink/25">
            {items.map((e) => (
              <HistoryRow key={e.id} entry={e} onDelete={() => onDelete(e.id)} />
            ))}
          </ul>
        </div>
      ))}
    </div>
  );
}

function HistoryRow({ entry, onDelete }: { entry: Entry; onDelete: () => void }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    await api.copyText(entry.text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1200);
  };
  return (
    <li className="group flex items-start gap-5 py-3">
      <span className="w-16 shrink-0 pt-0.5 font-mono text-[12px] text-faint">{formatTime(entry.createdAt)}</span>
      <p className="selectable flex-1 text-[14px] leading-relaxed text-ink">
        {entry.text}
        {!entry.pasted && (
          <span className="ml-2 border-2 border-ink px-1.5 py-px align-middle text-[10px] font-bold tracking-wide uppercase">
            not pasted
          </span>
        )}
      </p>
      <span className="flex shrink-0 gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
        <IconButton label="Copy" onClick={copy}>
          {copied ? <Check size={15} className="text-ok" /> : <Copy size={15} />}
        </IconButton>
        <IconButton label="Delete" onClick={onDelete} tone="danger">
          <Trash2 size={15} />
        </IconButton>
      </span>
    </li>
  );
}
