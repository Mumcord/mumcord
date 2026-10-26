/**
 * Which links of a message get a preview card, which images are shown inline, the shared
 * lookups and the rules for embedding video players (FR-1.49, FR-1.50, FR-1.55).
 */
import { getApi, type InlineImage, type LinkPreview, type VideoEmbed } from "../../api";

/** More cards than this per message would bury the conversation. */
export const MAX_PREVIEWS = 3;

/** Origins the embedded players may load from: Spotify in the app window (CSP `frame-src`), the
 * others in the loopback player page. */
const PLAYER_ORIGINS = new Set([
  "https://www.youtube-nocookie.com",
  "https://player.twitch.tv",
  "https://clips.twitch.tv",
  "https://open.spotify.com",
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

/** File name of the image link for labels, or its host when the path has none. */
export function imageName(url: string): string {
  try {
    const parsed = new URL(url);
    const last = parsed.pathname.split("/").pop() ?? "";
    try {
      return decodeURIComponent(last) || parsed.hostname;
    } catch {
      return last || parsed.hostname;
    }
  } catch {
    return url;
  }
}

/** Largest box an image is shown in. */
export const IMAGE_BOX = { width: 400, height: 300 } as const;

/** What a click on an inline image does (the image viewer); `opener` is the clicked button. */
export type ImageClickHandler = (image: InlineImage, opener: HTMLElement) => void;

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

/** Only images the engine produced; anything else would make the webview load from the web. */
export function safeImage(src: string | null): string | null {
  return src !== null && /^data:image\/(png|jpeg);base64,/.test(src) ? src : null;
}

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

/**
 * How a card's player runs: Spotify's embed directly in the app window (it needs no referrer, so
 * it also runs on Tauri's custom `tauri://` scheme); YouTube and Twitch through the app's
 * loopback player page (FR-2.41a), because YouTube refuses to play without an http(s) referrer
 * and Twitch only accepts an https or `localhost` parent.
 */
export type CardPlayer = { via: "direct" | "page"; url: string };

/** The player of a video card, or `null` when the URL is not one of the known players. */
export function cardPlayer(video: VideoEmbed): CardPlayer | null {
  let url: URL;
  try {
    url = new URL(video.playerUrl);
  } catch {
    return null;
  }
  if (!PLAYER_ORIGINS.has(url.origin)) return null;
  if (url.origin === "https://open.spotify.com") {
    // Only the embed path is in the CSP; the rest of open.spotify.com is not a player.
    return url.pathname.startsWith("/embed/") ? { via: "direct", url: url.toString() } : null;
  }
  return { via: "page", url: url.toString() };
}
