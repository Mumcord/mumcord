/**
 * Avatar colours: the stable name colour, and the banner colour of the full profile (FR-1.63),
 * the average colour of the avatar image so the card picks up the person's look.
 */
import { useEffect, useState } from "react";

const PALETTE_SIZE = 8;

/** Stable per-name palette index (FNV-1a), so a user keeps their color everywhere. */
function avatarColorIndex(name: string): number {
  let hash = 0x811c9dc5;
  for (const ch of name.trim().toLowerCase()) {
    hash = Math.imul(hash ^ (ch.codePointAt(0) ?? 0), 0x01000193) >>> 0;
  }
  return (hash % PALETTE_SIZE) + 1;
}

/** The person's name colour as a CSS value (the avatar background without an image). */
export function avatarColorVar(name: string): string {
  return `var(--color-avatar-${String(avatarColorIndex(name))})`;
}

/** The avatar is scaled down to this many pixels per side before averaging. */
const SAMPLE_SIZE = 16;

/** Average of the visible pixels of RGBA data as `rgb(r g b)`; `null` when all are transparent. */
export function averageColor(rgba: Uint8ClampedArray): string | null {
  let r = 0;
  let g = 0;
  let b = 0;
  let weight = 0;
  for (let i = 0; i + 3 < rgba.length; i += 4) {
    // Weighted by alpha, so transparent corners of round avatars don't pull the colour to black.
    const a = rgba[i + 3] ?? 0;
    r += (rgba[i] ?? 0) * a;
    g += (rgba[i + 1] ?? 0) * a;
    b += (rgba[i + 2] ?? 0) * a;
    weight += a;
  }
  if (weight === 0) return null;
  const channel = (sum: number) => String(Math.round(sum / weight));
  return `rgb(${channel(r)} ${channel(g)} ${channel(b)})`;
}

/** The average colour of the image at `src`, or `null` while loading, without image or on failure. */
export function useAvatarColor(src: string | null): string | null {
  const [color, setColor] = useState<{ src: string; value: string | null } | null>(null);

  useEffect(() => {
    if (src === null || src === "") return;
    let active = true;
    const image = new Image();
    image.onload = () => {
      if (!active) return;
      let value: string | null = null;
      try {
        const canvas = document.createElement("canvas");
        canvas.width = SAMPLE_SIZE;
        canvas.height = SAMPLE_SIZE;
        const context = canvas.getContext("2d");
        if (context) {
          context.drawImage(image, 0, 0, SAMPLE_SIZE, SAMPLE_SIZE);
          value = averageColor(context.getImageData(0, 0, SAMPLE_SIZE, SAMPLE_SIZE).data);
        }
      } catch (cause) {
        // The banner then uses the name colour; nothing the user needs to know about.
        console.warn("Reading the avatar colour failed", cause);
      }
      setColor({ src, value });
    };
    image.src = src;
    return () => {
      active = false;
    };
  }, [src]);

  return color !== null && color.src === src ? color.value : null;
}
