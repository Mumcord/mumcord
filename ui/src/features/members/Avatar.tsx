import { useState, type CSSProperties } from "react";
import "./members.css";

const PALETTE_SIZE = 8;

/** Stable per-name palette index (FNV-1a), so a user keeps their color everywhere. */
function avatarColorIndex(name: string): number {
  let hash = 0x811c9dc5;
  for (const ch of name.trim().toLowerCase()) {
    hash = Math.imul(hash ^ (ch.codePointAt(0) ?? 0), 0x01000193) >>> 0;
  }
  return (hash % PALETTE_SIZE) + 1;
}

/** Round avatar: the person's image when known and decodable, otherwise their initial. Decorative. */
export function Avatar({
  name,
  src = null,
  talking = false,
  size = "md",
}: {
  name: string;
  src?: string | null;
  talking?: boolean;
  size?: "sm" | "md" | "lg";
}) {
  // Remember which source failed to load; a new src gets a fresh attempt.
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const showImage = src !== null && src !== "" && src !== failedSrc;
  return (
    <span
      className="avatar"
      data-size={size}
      data-talking={talking}
      style={
        { "--avatar-bg": `var(--color-avatar-${String(avatarColorIndex(name))})` } as CSSProperties
      }
      aria-hidden="true"
    >
      {showImage ? (
        <img
          src={src}
          alt=""
          onError={() => {
            setFailedSrc(src);
          }}
        />
      ) : (
        (name.trim()[0] ?? "?").toUpperCase()
      )}
    </span>
  );
}
