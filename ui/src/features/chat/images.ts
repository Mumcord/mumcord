/**
 * Pasted/dropped images are sent inline as `data:` URLs, so they have to fit the server's image
 * message limit (FR-1.30). Large images are scaled down (as PNG or JPEG, whichever keeps more detail) until they fit;
 * if even the smallest version is too big, the image is refused.
 */
import type { ServerInfo } from "../../api";

/**
 * Used when the server reports 0 ("unlimited") for image messages: 1 MiB, small enough
 * that other clients do not choke on the message.
 */
export const UNLIMITED_IMAGE_FALLBACK = 1024 * 1024;

/** Largest edge of a sent image; bigger pictures are scaled down to at most this. */
export const MAX_IMAGE_EDGE = 1920;
/** Larger files are refused before reading; decoding them would stall the UI. */
export const MAX_SOURCE_BYTES = 25 * 1024 * 1024;
const MIN_IMAGE_EDGE = 64;
const JPEG_QUALITY = 0.85;

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

export type ImageFormat = "png" | "jpeg";

/** Encodes the image at the given size and returns a `data:` URL (quality only applies to JPEG). */
export type Encoder = (
  width: number,
  height: number,
  format: ImageFormat,
  quality: number,
) => Promise<string>;

/** Smallest steps of the edge search: stops when the bracket is this fine (fraction of the edge). */
const SEARCH_PRECISION = 0.04;

async function largestFit(
  size: { width: number; height: number },
  maxEdge: number,
  encode: (width: number, height: number) => Promise<string>,
  budget: number,
): Promise<{ edge: number; url: string } | null> {
  const longest = Math.max(size.width, size.height);
  const attempt = async (edge: number) => {
    const scale = edge / longest;
    const url = await encode(
      Math.max(1, Math.round(size.width * scale)),
      Math.max(1, Math.round(size.height * scale)),
    );
    return url.length <= budget ? url : null;
  };
  const top = await attempt(maxEdge);
  if (top !== null) return { edge: maxEdge, url: top };
  let best: { edge: number; url: string } | null = null;
  let low = MIN_IMAGE_EDGE;
  let high = maxEdge;
  // Binary search on the edge: about six encodes find the largest size within a few percent.
  while (high - low > Math.max(8, high * SEARCH_PRECISION)) {
    const edge = Math.round((low + high) / 2);
    const url = await attempt(edge);
    if (url !== null) {
      best = { edge, url };
      low = edge;
    } else {
      high = edge;
    }
  }
  if (best === null) {
    const url = await attempt(low);
    if (url !== null) best = { edge: low, url };
  }
  return best;
}

/**
 * Finds the largest encoding whose `data:` URL is at most `budget` characters. PNG (lossless,
 * best for screenshots) wins when it fits at least as large as the JPEG; otherwise JPEG
 * (best for photos). Returns `null` when nothing fits.
 */
export async function fitImage(
  size: { width: number; height: number },
  encode: Encoder,
  budget: number,
  maxEdge: number = MAX_IMAGE_EDGE,
): Promise<{ url: string; width: number; height: number } | null> {
  const longest = Math.max(size.width, size.height);
  const cap = Math.min(maxEdge, longest);
  const png = await largestFit(size, cap, (w, h) => encode(w, h, "png", 1), budget);
  // A full-size PNG that fits cannot be beaten, so skip the JPEG search.
  const jpeg =
    png !== null && png.edge === cap
      ? null
      : await largestFit(size, cap, (w, h) => encode(w, h, "jpeg", JPEG_QUALITY), budget);
  const winner = png !== null && (jpeg === null || png.edge >= jpeg.edge) ? png : jpeg;
  if (winner === null) return null;
  const scale = winner.edge / longest;
  return {
    url: winner.url,
    width: Math.max(1, Math.round(size.width * scale)),
    height: Math.max(1, Math.round(size.height * scale)),
  };
}

export type ImageResult =
  | { ok: true; dataUrl: string; width: number; height: number; scaled: boolean }
  | { ok: false; reason: "notImage" | "unreadable" | "tooLarge" | "sourceTooLarge" };

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
  return async (width, height, format, quality) => {
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d");
    if (!context) throw new Error("canvas unavailable");
    if (format === "jpeg") {
      // JPEG has no transparency; a white background matches how most clients show PNGs.
      context.fillStyle = "#ffffff";
      context.fillRect(0, 0, width, height);
    }
    context.imageSmoothingQuality = "high";
    context.drawImage(image, 0, 0, width, height);
    // toBlob encodes off the main thread, so the UI stays responsive.
    const blob = await new Promise<Blob | null>((resolve) => {
      canvas.toBlob(resolve, `image/${format}`, quality);
    });
    if (!blob) throw new Error("encoding failed");
    return readAsDataUrl(blob);
  };
}

/**
 * Prepares a file for sending within `budget` characters. Small images that already fit are
 * kept as they are (keeps GIF animation and PNG transparency).
 */
export async function prepareImage(file: Blob, budget: number): Promise<ImageResult> {
  if (!SENDABLE_TYPES.includes(file.type)) return { ok: false, reason: "notImage" };
  if (file.size > MAX_SOURCE_BYTES) return { ok: false, reason: "sourceTooLarge" };
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
    return { ok: true, dataUrl: original, width, height, scaled: false };
  }
  try {
    const fitted = await fitImage({ width, height }, canvasEncoder(image), budget);
    if (!fitted) return { ok: false, reason: "tooLarge" };
    return {
      ok: true,
      dataUrl: fitted.url,
      width: fitted.width,
      height: fitted.height,
      scaled: fitted.width !== width || fitted.height !== height,
    };
  } catch (error) {
    console.error("Compressing an image for a chat message failed", error);
    return { ok: false, reason: "unreadable" };
  }
}

/** The `<img>` tag as the official client sends it. */
export function imageTag(dataUrl: string): string {
  return `<img src="${dataUrl}" />`;
}
