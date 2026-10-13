import type { ButtonHTMLAttributes, ReactNode } from "react";
import { ChevronDown } from "lucide-react";

/*
 * Brutalist primitives: square corners, 2px
 * near-black borders, white boxes on a warm off-white grid, uppercase bold
 * labels, and hover states that invert to solid black.
 */

export function Card({
  title,
  action,
  children,
  className = "",
  id,
}: {
  title?: string;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
  id?: string;
}) {
  return (
    <section id={id} className={`rise border-2 border-ink bg-card p-5 ${className}`}>
      {(title || action) && (
        <header className="mb-4 flex items-center justify-between gap-3">
          {title && <h2 className="type-display text-[18px]">{title}</h2>}
          {action}
        </header>
      )}
      {children}
    </section>
  );
}

/** Small uppercase label, like the site's SEARCH / COUNTRY headings. */
export function Label({ children, htmlFor }: { children: ReactNode; htmlFor?: string }) {
  return (
    <label htmlFor={htmlFor} className="block text-[12px] font-bold tracking-wide text-ink uppercase">
      {children}
    </label>
  );
}

/** Keyboard key chips, e.g. [CTRL] + [WIN]. */
export function Keys({
  labels,
  size = "md",
  tone = "default",
}: {
  labels: string[];
  /** `hero` scales with the surrounding headline. */
  size?: "md" | "lg" | "hero";
  /** `accent` renders coral chips for the hero. */
  tone?: "default" | "accent";
}) {
  const chip = {
    md: "min-w-9 px-2 py-0.5 text-[12px]",
    lg: "min-w-14 px-3 py-1 text-[16px]",
    hero: "min-w-[2.4em] px-[0.35em] py-[0.08em] text-[0.68em]",
  }[size];
  const plus = size === "hero" ? "text-[0.45em]" : "text-xs";
  return (
    <span className="inline-flex items-center gap-2">
      {labels.map((label, i) => (
        <span key={label} className="inline-flex items-center gap-2">
          {i > 0 && <span className={`font-mono font-bold text-ink ${plus}`}>+</span>}
          <kbd
            className={`${chip} inline-flex items-center justify-center border-2 border-ink font-mono font-bold tracking-normal uppercase ${
              tone === "accent" ? "bg-coral text-ink" : "bg-card text-ink"
            }`}
          >
            {label}
          </kbd>
        </span>
      ))}
    </span>
  );
}

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "ghost" | "danger";
  icon?: ReactNode;
};

export function Button({ variant = "ghost", icon, children, className = "", ...rest }: ButtonProps) {
  const styles = {
    // Solid black, like the site's SUBSCRIBE button; inverts on hover.
    primary: "bg-ink text-on-ink hover:bg-card hover:text-ink",
    ghost: "bg-card text-ink hover:bg-ink hover:text-on-ink",
    danger: "bg-card text-danger hover:bg-danger hover:text-white hover:border-danger",
  }[variant];
  return (
    <button
      className={`inline-flex h-10 cursor-pointer items-center justify-center gap-2 border-2 border-ink px-4 text-[12px] font-bold tracking-wide uppercase transition-colors disabled:cursor-default disabled:opacity-50 ${styles} ${className}`}
      {...rest}
    >
      {icon}
      {children}
    </button>
  );
}

export function IconButton({
  label,
  children,
  onClick,
  tone = "default",
}: {
  label: string;
  children: ReactNode;
  onClick: () => void;
  tone?: "default" | "danger";
}) {
  return (
    <button
      aria-label={label}
      title={label}
      onClick={onClick}
      className={`flex size-8 cursor-pointer items-center justify-center border-2 border-transparent text-ink transition-colors hover:border-ink ${
        tone === "danger" ? "hover:bg-danger hover:text-white" : "hover:bg-ink hover:text-on-ink"
      }`}
    >
      {children}
    </button>
  );
}

/** Dropdown with a leading icon and a custom chevron, used for every select in the app. */
export function Select({
  id,
  value,
  onChange,
  icon,
  children,
  className = "",
}: {
  id?: string;
  value: string;
  onChange: (value: string) => void;
  icon: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={`relative ${className}`}>
      <span className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-ink">{icon}</span>
      <select
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="focus-ring h-10 w-full cursor-pointer appearance-none border-2 border-ink bg-card pr-9 pl-9 text-[13.5px] font-medium text-ink"
      >
        {children}
      </select>
      <ChevronDown size={16} className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 text-ink" />
    </div>
  );
}

/** Square switch: a bordered track with a square knob that slides across. */
export function Toggle({
  checked,
  onChange,
  label,
  description,
  disabled = false,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  label: string;
  description?: string;
  disabled?: boolean;
}) {
  return (
    <label
      className={`flex items-center justify-between gap-6 py-3.5 ${disabled ? "cursor-default opacity-50" : "cursor-pointer"}`}
    >
      <span>
        <span className="block text-[14px] font-semibold text-ink">{label}</span>
        {description && <span className="mt-0.5 block text-[12.5px] text-muted">{description}</span>}
      </span>
      <button
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={`focus-ring relative h-6 w-11 shrink-0 cursor-pointer border-2 border-ink transition-colors disabled:cursor-default ${
          checked ? "bg-ink" : "bg-card"
        }`}
      >
        <span
          className={`absolute top-[2px] size-4 transition-all ${checked ? "left-[21px] bg-on-ink" : "left-[2px] bg-ink"}`}
        />
      </button>
    </label>
  );
}
