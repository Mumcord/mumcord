/**
 * "Copy image" and "Save image…" for chat images (issue #115), shared by the image context menu
 * and the image viewer. Both work on the bytes the chat already shows (the full-size image once
 * it has arrived): nothing is fetched from the web for them. The engine decodes a copied image to
 * plain pixels for the system clipboard, so a GIF copies as its first frame, while saving writes
 * the original file, animation included.
 */
import { EngineError, getApi } from "../../api";
import { pickSavePath } from "../settings/fileDialogs";

/** Images the engine produced; anything else is not ours to copy or save. */
const IMAGE_DATA_URL = /^data:image\/(png|jpeg|gif|webp);base64,/;

/** Largest image copied or saved; the engine checks the same limit (`MAX_IMAGE_BYTES`). */
export const MAX_IMAGE_BYTES = 50 * 1024 * 1024;

export class ImageTooLargeError extends Error {
  constructor() {
    super("image too large");
  }
}

/** User-facing text for a failed copy or save; the log gets the error, never the image or path. */
export function imageActionError(error: unknown, failed: string, tooLarge: string): string {
  const large =
    error instanceof ImageTooLargeError ||
    (error instanceof EngineError && error.code === "messageTooLarge");
  return large ? tooLarge : failed;
}

function readAsDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      resolve(typeof reader.result === "string" ? reader.result : "");
    };
    reader.onerror = () => {
      reject(reader.error ?? new Error("reading the image failed"));
    };
    reader.readAsDataURL(blob);
  });
}

/**
 * The `data:` URL of a shown image. A `blob:` URL is read back from memory; every other source
 * is refused, so this can never reach the network.
 */
export async function imageDataUrl(src: string): Promise<string> {
  if (IMAGE_DATA_URL.test(src)) {
    if ((src.length / 4) * 3 > MAX_IMAGE_BYTES + 3) throw new ImageTooLargeError();
    return src;
  }
  if (!src.startsWith("blob:")) throw new Error("not a local image");
  const blob = await (await fetch(src)).blob();
  if (blob.size > MAX_IMAGE_BYTES) throw new ImageTooLargeError();
  const dataUrl = await readAsDataUrl(blob);
  if (!IMAGE_DATA_URL.test(dataUrl)) throw new Error("not a supported image");
  return dataUrl;
}

/** File extension of the image's real format. */
export function imageExtension(dataUrl: string): string {
  const format = IMAGE_DATA_URL.exec(dataUrl)?.[1] ?? "png";
  return format === "jpeg" ? "jpg" : format;
}

function day(now: Date): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${String(now.getFullYear())}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

/**
 * Suggested file name: the image's own name (`null` when unknown) with the extension of its
 * real format, so a GIF link that delivered a WebP saves as `.webp`; else `image-<date>.<ext>`.
 */
export function imageFileName(name: string | null, extension: string, now = new Date()): string {
  const cleaned = (name ?? "").replace(/[\\/:*?"<>|]/g, "_").trim();
  const base = cleaned.replace(/\.(?:png|jpe?g|gif|webp)$/i, "");
  return `${base === "" ? `image-${day(now)}` : base}.${extension}`;
}

/** Puts the image on the system clipboard as image data. */
export async function copyImage(src: string): Promise<void> {
  await getApi().copyImage(await imageDataUrl(src));
}

/**
 * Asks where to save the image and writes its original bytes; `false` when the user cancels.
 * `title` is the dialog's title, `filterName` the name of the file type filter.
 */
export async function saveImageAs(
  src: string,
  name: string | null,
  title: string,
  filterName: string,
): Promise<boolean> {
  const dataUrl = await imageDataUrl(src);
  const extension = imageExtension(dataUrl);
  const path = await pickSavePath(
    imageFileName(name, extension),
    { name: filterName, extensions: [extension] },
    title,
  );
  if (path === null) return false;
  await getApi().saveImage(path, dataUrl);
  return true;
}
