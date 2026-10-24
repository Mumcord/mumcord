/**
 * Which links of a message get a preview card, which images are shown inline, the shared
 * lookups and the rules for embedding video players (FR-1.49, FR-1.50, FR-1.55).
 */
import { getApi, type InlineImage, type LinkPreview, type VideoEmbed } from "../../api";

/** More cards than this per message would bury the conversation. */
export const MAX_PREVIEWS = 3;

/** Origins the embedded players may load from; the CSP `frame-src` allows exactly these. */
const PLAYER_ORIGINS = new Set([
  "https://www.youtube-nocookie.com",
  "https://player.twitch.tv",
  "https://clips.twitch.tv",
]);

/** Distinct http(s) link targets of sanitized message HTML, in order, at most {@link MAX_PREVIEWS}. */
export function previewableLinks(html: string): string[] {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const links: string[] = [];
  for (const anchor of doc.querySelectorAll("a[href]")) {
    const href = anchor.getAttribute("href")?.trim() ?? "";
    if (!/^https?:\/\//i.test(href) || links.includes(href)) continue;
    links.push(href);
    if (links.length === MAX_PREVIEWS) break;
  }
  return links;
}

/** Direct image links: the path ends in an image file extension (FR-1.55). */
export function isImageLink(url: string): boolean {
  try {
    return /\.(?:png|jpe?g|gif|webp)$/i.test(new URL(url).pathname);
  } catch {
    return false;
  }
}

/**
 * Distinct http(s) image sources (`<img src>`) of the engine-sanitized message HTML, in order,
 * at most {@link MAX_PREVIEWS}. The UI sanitizer removes these images from the message text;
 * they are shown inline through the engine instead (FR-1.55). Parsing happens in an inert
 * document, so nothing is loaded here.
 */
export function externalImages(html: string): string[] {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const images: string[] = [];
  for (const img of doc.querySelectorAll("img[src]")) {
    const src = img.getAttribute("src")?.trim() ?? "";
    if (!/^https?:\/\//i.test(src) || images.includes(src)) continue;
    images.push(src);
    if (images.length === MAX_PREVIEWS) break;
  }
  return images;
}

/** Largest box an image is shown in. */
export const IMAGE_BOX = { width: 400, height: 300 } as const;

/** What a click on an inline image does; the image viewer (B42) plugs in here. */
export type ImageClickHandler = (image: InlineImage) => void;

/** Display size of a `width`×`height` image inside {@link IMAGE_BOX}; never scaled up. */
export function fitImage(width: number, height: number): { width: number; height: number } {
  const scale = Math.min(1, IMAGE_BOX.width / width, IMAGE_BOX.height / height);
  return {
    width: Math.max(1, Math.round(width * scale)),
    height: Math.max(1, Math.round(height * scale)),
  };
}

/** Requests `key` once per app run; failures are logged and retried on the next request. */
function memoized<T>(
  cache: Map<string, Promise<T | null>>,
  key: string,
  what: string,
  load: () => Promise<T | null>,
): Promise<T | null> {
  let result = cache.get(key);
  if (!result) {
    result = load().catch((error: unknown) => {
      console.error(`Loading ${what} failed`, error);
      cache.delete(key);
      return null;
    });
    cache.set(key, result);
  }
  return result;
}

const previews = new Map<string, Promise<LinkPreview | null>>();
const images = new Map<string, Promise<InlineImage | null>>();

/** The engine's preview of `url`, requested once per app run (the engine caches across runs). */
export function loadPreview(url: string): Promise<LinkPreview | null> {
  return memoized(previews, url, "a link preview", () => getApi().linkPreview(url));
}

/** The engine's inline image for `url`, requested once per app run (the engine caches across runs). */
export function loadInlineImage(url: string): Promise<InlineImage | null> {
  return memoized(images, url, "an inline image", () => getApi().inlineImage(url));
}

/** Forgets loaded previews and images (tests). */
export function clearPreviews(): void {
  previews.clear();
  images.clear();
}

interface PageLocation {
  protocol: string;
  hostname: string;
}

/**
 * Player URL for an embed in this window, or `null` when the player cannot run here and the
 * card opens the browser instead. YouTube refuses to play without an http(s) referrer, which
 * pages served from Tauri's custom `tauri://` scheme (Linux, macOS) cannot send. Twitch only
 * accepts a `parent` that is served over HTTPS or is `localhost`.
 */
export function playerUrl(
  video: VideoEmbed,
  location: PageLocation = window.location,
): string | null {
  const { protocol, hostname } = location;
  if (protocol !== "http:" && protocol !== "https:") return null;
  let url: URL;
  try {
    url = new URL(video.playerUrl);
  } catch {
    return null;
  }
  if (!PLAYER_ORIGINS.has(url.origin)) return null;
  if (video.site === "twitch") {
    if (protocol !== "https:" && hostname !== "localhost") return null;
    url.searchParams.set("parent", hostname);
  }
  return url.toString();
}
