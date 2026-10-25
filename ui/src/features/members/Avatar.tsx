import { useState, type CSSProperties } from "react";
import "./members.css";
import { avatarColorVar } from "./avatarColor";

/**
 * Round avatar: the person's image when known and decodable, otherwise their initial. Decorative.
 * The initial is drawn by CSS from `data-initial`, not as DOM text: rows named after the person
 * (aria-labelledby) would otherwise show visible text that is not in their name (WCAG 2.5.3).
 */
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
      data-initial={showImage ? undefined : (name.trim()[0] ?? "?").toUpperCase()}
      style={{ "--avatar-bg": avatarColorVar(name) } as CSSProperties}
      aria-hidden="true"
    >
      {showImage && (
        <img
          src={src}
          alt=""
          onError={() => {
            setFailedSrc(src);
          }}
        />
      )}
    </span>
  );
}
