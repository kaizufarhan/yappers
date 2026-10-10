/**
 * The Yappers mark: a one-line doodle of a face in profile (forehead, nose,
 * open mouth, chin) with three speech lines. The face uses `currentColor`;
 * the speech lines are coral. Same artwork as src-tauri/icons/yappers.svg.
 */
export function Logo({ size = 24, className = "" }: { size?: number; className?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={1.75}
      aria-hidden="true"
      className={className}
    >
      <path
        stroke="currentColor"
        d="M5 1.8c2 1.6 2.7 3.8 2.2 6.2 3.4.3 6.2 2 6.1 4.2-.1 1.2-1.5 1.6-3.2 1.3l1.8 1.7-2.6 1.4 2.6 1.3c.6 1.7-.4 3.1-2.5 3.6-1.3.3-2.1 1.1-2.5 2.3"
      />
      <path stroke="var(--color-coral)" d="M14.6 13.6 17.5 12M15.2 16.1H19M14.6 18.6l2.9 1.6" />
    </svg>
  );
}
