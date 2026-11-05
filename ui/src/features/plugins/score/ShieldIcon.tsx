/** Which symbol a score label carries next to its colour and words (FR-9.71). */
export type ShieldKind = "check" | "exclamation" | "cross" | "blocked";

const MARKS: Record<Exclude<ShieldKind, "blocked">, string> = {
  check: "m9 12 2 2 4-4",
  exclamation: "M12 8v4M12 16h.01",
  cross: "m9.5 9.5 5 5m0-5-5 5",
};

/** Decorative shield (or "no entry" circle); the words next to it carry the meaning. */
export function ShieldIcon({ kind, size }: { kind: ShieldKind; size: number }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      aria-hidden="true"
      focusable="false"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {kind === "blocked" ? (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="m5.6 5.6 12.8 12.8" />
        </>
      ) : (
        <>
          <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
          <path d={MARKS[kind]} />
        </>
      )}
    </svg>
  );
}
