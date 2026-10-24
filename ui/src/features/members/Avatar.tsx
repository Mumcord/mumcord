import { useState } from "react";
import "./members.css";

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
    <span className="avatar" data-size={size} data-talking={talking} aria-hidden="true">
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
