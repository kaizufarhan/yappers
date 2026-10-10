import { useEffect, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { Logo } from "./Logo";

/**
 * Custom title bar replacing the Windows one, styled like the site's top nav:
 * a bar with a 2px rule underneath and segments split by vertical rules that
 * invert to black on hover. The bar itself is a drag region (double-click
 * maximizes); closing hides Yappers to the tray, like the native button did.
 */
export function TitleBar({ children }: { children?: ReactNode }) {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    const sync = () => win.isMaximized().then(setMaximized).catch(() => {});
    sync();
    const sub = win.onResized(sync);
    return () => void sub.then((unlisten) => unlisten());
  }, []);

  const win = getCurrentWindow();

  return (
    <header
      data-tauri-drag-region
      className="flex h-11 shrink-0 items-stretch justify-between border-b-2 border-ink bg-canvas select-none"
    >
      {/* Decorative children ignore the pointer so drags reach the bar. */}
      <div className="pointer-events-none flex items-center gap-2.5 pl-4">
        <span className="flex size-7 items-center justify-center bg-ink text-on-ink">
          <Logo size={22} />
        </span>
        <span className="type-display text-[17px]">Yappers</span>
      </div>

      <div className="flex">
        <div className="pointer-events-none flex items-center border-l-2 border-ink px-4">{children}</div>
        <WindowButton label="Minimize" onClick={() => win.minimize()}>
          <Minus size={16} strokeWidth={2} />
        </WindowButton>
        <WindowButton label={maximized ? "Restore" : "Maximize"} onClick={() => win.toggleMaximize()}>
          {maximized ? <Copy size={13} strokeWidth={2} className="-scale-x-100" /> : <Square size={13} strokeWidth={2} />}
        </WindowButton>
        <WindowButton label="Close" onClick={() => win.close()} close>
          <X size={17} strokeWidth={2} />
        </WindowButton>
      </div>
    </header>
  );
}

function WindowButton({
  label,
  onClick,
  close,
  children,
}: {
  label: string;
  onClick: () => void;
  close?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      aria-label={label}
      title={label}
      onClick={onClick}
      className={`flex w-12 items-center justify-center border-l-2 border-ink text-ink transition-colors ${
        // Windows convention: close turns red on hover; the rest invert.
        close ? "hover:bg-[#c42b1c] hover:text-white" : "hover:bg-ink hover:text-on-ink"
      }`}
    >
      {children}
    </button>
  );
}
