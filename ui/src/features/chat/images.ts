/**
 * Pasted/dropped images are sent inline as `data:` URLs, so they have to fit the server's image
 * message limit (FR-1.30). Large images are scaled down and re-encoded as JPEG until they fit;
 * if even the smallest version is too big, the image is refused.
 */
import type { ServerInfo } from "../../api";

/**
 * Used when the server reports 0 ("unlimited") for image messages: 128 KiB, the default
 * `imagemessagelength` of the Mumble server. Keeps messages small enough for every client.
 */
export const UNLIMITED_IMAGE_FALLBACK = 128 * 1024;

/** Largest edge of a sent image; bigger pictures are scaled down first. */
export const MAX_IMAGE_EDGE = 600;
const MIN_IMAGE_EDGE = 64;
const QUALITIES = [0.85, 0.7, 0.55, 0.4];
const SHRINK = 0.75;

const SENDABLE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/** Size limits of the connected server, in characters (0 = unlimited for text). */
export interface MessageLimits {
  text: number;
  image: number;
  allowHtml: boolean;
}

export function messageLimits(info: ServerInfo | null): MessageLimits {
  return {
    text: info?.maxMessageLength ?? 0,
    image:
      info && info.maxImageMessageLength > 0
        ? info.maxImageMessageLength
        : UNLIMITED_IMAGE_FALLBACK,
    allowHtml: info?.allowHtml ?? true,
  };
}

/** Encodes the image at the given size and JPEG quality and returns a `data:` URL. */
export type Encoder = (width: number, height: number, quality: number) => Promise<string>;

/**
 * Finds the largest, best-quality encoding whose `data:` URL is at most `budget` characters.
 * Returns `null` when nothing fits.
 */
export async function fitImage(
  size: { width: number; height: number },
  encode: Encoder,
  budget: number,
): Promise<string | null> {
  let edge = Math.min(MAX_IMAGE_EDGE, Math.max(size.width, size.height));
  while (edge >= MIN_IMAGE_EDGE) {
    const scale = edge / Math.max(size.width, size.height);
    const width = Math.max(1, Math.round(size.width * scale));
    const height = Math.max(1, Math.round(size.height * scale));
    for (const quality of QUALITIES) {
      const url = await encode(width, height, quality);
      if (url.length <= budget) return url;
    }
    edge = Math.floor(edge * SHRINK);
  }
  return null;
}

export type ImageResult =
  { ok: true; dataUrl: string } | { ok: false; reason: "notImage" | "unreadable" | "tooLarge" };

function readAsDataUrl(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      resolve(typeof reader.result === "string" ? reader.result : "");
    };
    reader.onerror = () => {
      reject(reader.error ?? new Error("reading the image failed"));
    };
    reader.readAsDataURL(file);
  });
}

async function decode(url: string): Promise<HTMLImageElement> {
  const image = new Image();
  image.src = url;
  await image.decode();
  return image;
}

function canvasEncoder(image: HTMLImageElement): Encoder {
  return (width, height, quality) => {
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d");
    if (!context) return Promise.reject(new Error("canvas unavailable"));
    // JPEG has no transparency; a white background matches how most clients show PNGs.
    context.fillStyle = "#ffffff";
    context.fillRect(0, 0, width, height);
    context.drawImage(image, 0, 0, width, height);
    return Promise.resolve(canvas.toDataURL("image/jpeg", quality));
  };
}

/**
 * Prepares a file for sending within `budget` characters. Small images that already fit are
 * kept as they are (keeps GIF animation and PNG transparency).
 */
export async function prepareImage(file: Blob, budget: number): Promise<ImageResult> {
  if (!SENDABLE_TYPES.includes(file.type)) return { ok: false, reason: "notImage" };
  let original: string;
  let image: HTMLImageElement;
  try {
    original = await readAsDataUrl(file);
    image = await decode(original);
  } catch (error) {
    console.error("Reading an image for a chat message failed", error);
    return { ok: false, reason: "unreadable" };
  }
  const { naturalWidth: width, naturalHeight: height } = image;
  if (Math.max(width, height) <= MAX_IMAGE_EDGE && original.length <= budget) {
    return { ok: true, dataUrl: original };
  }
  try {
    const fitted = await fitImage({ width, height }, canvasEncoder(image), budget);
    return fitted ? { ok: true, dataUrl: fitted } : { ok: false, reason: "tooLarge" };
  } catch (error) {
    console.error("Compressing an image for a chat message failed", error);
    return { ok: false, reason: "unreadable" };
  }
}

/** The `<img>` tag as the official client sends it. */
export function imageTag(dataUrl: string): string {
  return `<img src="${dataUrl}" />`;
}
