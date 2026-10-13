import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Check, X, AlertCircle, LoaderCircle } from "lucide-react";
import { AnimatePresence, MotionConfig, motion } from "motion/react";
import { api, on, type OverlaySnapshot, type OverlayState } from "../lib/api";

/** One word of the live transcript. `batch` is the update that first added it. */
interface Word {
  id: number;
  text: string;
  batch: number;
}

interface LiveText {
  session: number;
  text: string;
}

/** Words beyond this are dropped; only the most recent few fit in the bubble. */
const MAX_WORDS = 12;

const norm = (w: string) => w.toLowerCase().replace(/[^\p{L}\p{N}']/gu, "");

/**
 * Merges a fresh transcript of the last few seconds into the words on screen.
 * The preview window slides, so the new text overlaps the old: words up to the
 * last previously-shown word keep their identity (and colour), anything after
 * it is new speech and is highlighted.
 */
function merge(prev: Word[], text: string, nextId: () => number, batch: number): { words: Word[]; fresh: boolean } {
  const incoming = text.split(/\s+/).filter(Boolean);
  if (prev.length === 0) {
    return { words: incoming.map((t) => ({ id: nextId(), text: t, batch })), fresh: incoming.length > 0 };
  }
  const anchor = norm(prev[prev.length - 1].text);
  let j = -1;
  for (let i = incoming.length - 1; i >= 0; i--) {
    if (norm(incoming[i]) === anchor) {
      j = i;
      break;
    }
  }
  // If the anchor fell out of the window, treat only the last word as new.
  if (j === -1) j = incoming.length - 2;

  const words: Word[] = incoming.map((t, i) => {
    if (i <= j) {
      // Align from the anchor backwards onto previously shown words.
      const old = prev[prev.length - 1 - (j - i)];
      return old ? { ...old, text: t } : { id: nextId(), text: t, batch: -1 };
    }
    return { id: nextId(), text: t, batch };
  });
  // Keep words that have slid out of the preview window, so the line reads
  // continuously instead of losing its start every few seconds.
  const kept = prev.slice(0, Math.max(0, prev.length - 1 - j));
  return { words: [...kept, ...words].slice(-MAX_WORDS), fresh: j < incoming.length - 1 };
}

/** The floating dictation bubble shown at the bottom of the screen. */
export function Overlay() {
  const [state, setState] = useState<OverlayState>({ state: "hidden" });
  const [words, setWords] = useState<Word[]>([]);
  const [batch, setBatch] = useState(0);
  const revision = useRef(-1);
  const session = useRef(-1);
  const ids = useRef(0);
  const batchRef = useRef(0);
  const wordsRef = useRef<Word[]>([]);
  const closedSession = useRef(-1);
  // Bumped each time the bubble opens, so every opening starts from a fresh tree.
  // While the window is hidden it gets no animation frames, so half-finished
  // exit/layout animations from the last dictation would otherwise flash in
  // their stale positions for a frame when it reappears.
  const [opening, setOpening] = useState(0);
  const visible = useRef(false);

  useEffect(() => {
    const apply = (snapshot: OverlaySnapshot) => {
      if (snapshot.revision < revision.current) return;
      revision.current = snapshot.revision;
      const nowVisible = snapshot.state.state !== "hidden";
      if (nowVisible && !visible.current) setOpening((n) => n + 1);
      visible.current = nowVisible;
      setState(snapshot.state);
      // Once a dictation is no longer listening, its words must not carry over
      // into the next one, including preview updates that arrive late.
      if (snapshot.state.state !== "recording" && snapshot.state.state !== "locked") {
        closedSession.current = Math.max(closedSession.current, session.current);
        wordsRef.current = [];
        setWords([]);
      }
    };
    const subs = [
      on<OverlaySnapshot>("overlay-state", apply),
      on<LiveText>("live-text", ({ session: s, text }) => {
        if (s <= closedSession.current) return;
        if (s !== session.current) {
          session.current = s;
          wordsRef.current = [];
        }
        const next = merge(wordsRef.current, text, () => ++ids.current, batchRef.current + 1);
        if (next.fresh) batchRef.current += 1;
        wordsRef.current = next.words;
        setWords(next.words);
        setBatch(batchRef.current);
      }),
    ];
    // A shortcut can arrive before the webview has mounted its event listeners.
    // Read the current state after subscribing; revisions avoid stale updates.
    void subs[0].then(() => api.overlaySnapshot().then(apply)).catch(() => {});
    return () => subs.forEach((p) => p.then((unlisten) => unlisten()));
  }, []);

  const s = state.state;
  const listening = s === "recording" || s === "locked";
  // One key per thing the bubble can show, so AnimatePresence crossfades between them.
  const view = listening ? (words.length > 0 ? "words" : "listening") : s;

  const locked = s === "locked";

  return (
    <MotionConfig reducedMotion="user">
        <div className="relative h-screen w-screen overflow-hidden border border-white/10 bg-[#1e2028] font-[Figtree,sans-serif]">
          {/* Each opening fades its content in just behind the window's
              native unfold (see overlay.rs), so text never pops in first. */}
          <motion.div
            key={opening}
            className="absolute inset-0"
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.22, delay: 0.07, ease: EASE_OUT }}
          >
            {/* Buttons sit on the bubble's edges rather than in the flow, so the
                text area's box never moves when they appear. */}
            <AnimatePresence initial={false}>
              {locked && (
                <BubbleButton key="cancel" side="left" label="Cancel" onClick={() => api.overlayCancel()}>
                  <X size={15} strokeWidth={2.5} />
                </BubbleButton>
              )}
            </AnimatePresence>

            <div
              className={`absolute inset-0 flex items-center justify-center overflow-hidden ${
                locked ? "px-12" : "px-4"
              }`}
            >
              <AnimatePresence mode="popLayout" initial={false}>
                <motion.div
                  key={view}
                  className="flex w-full min-w-0 items-center justify-center"
                  initial={{ opacity: 0, y: 6, filter: "blur(4px)" }}
                  animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
                  exit={{ opacity: 0, y: -6, filter: "blur(4px)" }}
                  transition={{ duration: 0.16, ease: EASE_OUT }}
                >
                  {view === "words" && <LiveWords words={words} batch={batch} />}
                  {view === "listening" && <Hint>Yap away...</Hint>}
                  {view === "processing" && <Pasting />}
                  {state.state === "error" && (
                    <span className="flex min-w-0 items-center gap-1.5 text-[13px] text-[#fff8f2]" title={state.message}>
                      <AlertCircle size={15} className="shrink-0 text-[#ff8f7a]" />
                      <span className="truncate">{state.message}</span>
                    </span>
                  )}
                </motion.div>
              </AnimatePresence>
            </div>

            <AnimatePresence initial={false}>
              {locked && (
                <BubbleButton key="finish" side="right" label="Finish" onClick={() => api.overlayStop()} primary>
                  <Check size={15} strokeWidth={3} />
                </BubbleButton>
              )}
            </AnimatePresence>
          </motion.div>
        </div>
    </MotionConfig>
  );
}

/** Soft spring for anything that moves or resizes. */
const SPRING = { type: "spring", stiffness: 420, damping: 38, mass: 0.7 } as const;
/** Quick start, gentle landing (easeOutQuint-like). */
const EASE_OUT = [0.22, 1, 0.36, 1] as const;
/** Gap between consecutive words revealed by the same preview pass. */
const STAGGER = 0.035;

/**
 * The latest words in white, earlier ones dimmed. Each preview pass can add
 * several words at once, so they're revealed one after another; words already
 * on screen glide aside on a spring instead of jumping as the line grows.
 * Long lines stay pinned to the newest word and fade out on the left.
 */
function LiveWords({ words, batch }: { words: Word[]; batch: number }) {
  const outer = useRef<HTMLDivElement>(null);
  const inner = useRef<HTMLDivElement>(null);
  const [overflowing, setOverflowing] = useState(false);

  useLayoutEffect(() => {
    if (outer.current && inner.current) {
      setOverflowing(inner.current.scrollWidth > outer.current.clientWidth);
    }
  }, [words]);

  const firstFresh = words.findIndex((w) => w.batch === batch);

  return (
    <div
      ref={outer}
      className={`flex w-full overflow-hidden ${overflowing ? "justify-end" : "justify-center"}`}
      // Always masked: text only reaches the left edge once it overflows, so
      // the fade never has to switch on abruptly.
      style={{ maskImage: "linear-gradient(to right, transparent, black 28px)" }}
    >
      <motion.div
        ref={inner}
        layout="position"
        transition={SPRING}
        className="flex shrink-0 gap-[0.28em] text-[15px] font-medium whitespace-nowrap"
      >
        <AnimatePresence initial={false} mode="popLayout">
          {words.map((w, i) => {
            const fresh = w.batch === batch;
            const delay = fresh && firstFresh >= 0 ? (i - firstFresh) * STAGGER : 0;
            return (
              <motion.span
                key={w.id}
                layout="position"
                initial={{ opacity: 0, y: 3, filter: "blur(4px)" }}
                animate={{ opacity: 1, y: 0, filter: "blur(0px)", color: fresh ? "#fff8f2" : "#a09d9d" }}
                exit={{ opacity: 0, filter: "blur(4px)", transition: { duration: 0.12 } }}
                transition={{
                  layout: SPRING,
                  // Mostly visible within ~0.1 s; the tail of the ease just settles.
                  opacity: { duration: 0.2, ease: EASE_OUT, delay },
                  y: { duration: 0.26, ease: EASE_OUT, delay },
                  filter: { duration: 0.2, ease: EASE_OUT, delay },
                  // Dim earlier words a little slower so the hand-off reads as one motion.
                  color: { duration: 0.5, ease: "easeOut" },
                }}
              >
                {w.text}
              </motion.span>
            );
          })}
        </AnimatePresence>
      </motion.div>
    </div>
  );
}

function Hint({ children }: { children: ReactNode }) {
  return <span className="text-[14px] font-medium text-[#b0acab]">{children}</span>;
}

function Pasting() {
  return (
    <span className="flex items-center gap-2 text-[14px] font-medium text-[#fff8f2]" role="status" aria-live="polite">
      <LoaderCircle size={15} className="motion-safe:animate-spin" aria-hidden="true" />
      Pasting…
    </span>
  );
}

function BubbleButton({
  children,
  side,
  label,
  onClick,
  primary,
}: {
  children: ReactNode;
  side: "left" | "right";
  label: string;
  onClick: () => void;
  primary?: boolean;
}) {
  return (
    <motion.button
      aria-label={label}
      title={label}
      onClick={onClick}
      initial={{ opacity: 0, scale: 0.5 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, scale: 0.5 }}
      whileTap={{ scale: 0.9 }}
      transition={SPRING}
      className={`absolute top-[7px] z-10 flex size-8 cursor-pointer items-center justify-center transition-colors ${
        side === "left" ? "left-[7px]" : "right-[7px]"
      } ${
        primary ? "bg-[#ff6b4a] text-[#1e2028] hover:brightness-105" : "bg-[#fff8f2]/15 text-[#fff8f2] hover:bg-[#fff8f2]/25"
      }`}
    >
      {children}
    </motion.button>
  );
}
