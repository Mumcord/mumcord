/**
 * Which links of a message get a preview card, which images are shown inline, the shared
 * lookups and the rules for embedding video and music players (FR-1.49, FR-1.50, FR-1.55,
 * issues #260 and #262).
 */
import {
  getApi,
  type AudioInfo,
  type InlineImage,
  type LinkPreview,
  type MumbleLinkStats,
  type MusicEmbed,
  type VideoEmbed,
} from "../../api";

/** More cards than this per message would bury the conversation. */
export const MAX_PREVIEWS = 3;

/** Origins the embedded video players may load from, all in the loopback player page. */
const PLAYER_ORIGINS = new Set([
  "https://www.youtube-nocookie.com",
  "https://player.twitch.tv",
  "https://clips.twitch.tv",
]);

/** Music embeds that run in the app window, by origin, with the only path the CSP `frame-src`
 * allows there. */
const DIRECT_MUSIC_PLAYERS = new Map([
  ["https://open.spotify.com", "/embed/"],
  ["https://w.soundcloud.com", "/player/"],
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

/** Direct audio links: the path ends in an audio file extension (issue #260). */
export function isAudioLink(url: string): boolean {
  try {
    return /\.(?:mp3|ogg|oga|opus|m4a|aac|wav|flac|weba|webm)$/i.test(new URL(url).pathname);
  } catch {
    return false;
  }
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
const audio = new Map<string, Promise<AudioInfo | null>>();

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

/**
 * Format and size of a direct audio link, asked once per app run; `refresh` asks again (the
 * card's Try again). The file itself is only fetched on play ({@link loadAudioFile}).
 */
export function loadAudioInfo(url: string, refresh = false): Promise<AudioInfo | null> {
  if (refresh) audio.delete(url);
  return memoized(audio, url, "audio file details", () => getApi().audioInfo(url));
}

/** The audio file as a `data:` URL, fetched by the engine; not kept, it can be large. */
export async function loadAudioFile(url: string): Promise<string | null> {
  try {
    const dataUrl = await getApi().audioFile(url);
    return dataUrl !== null && /^data:audio\/[a-z0-9.+-]+;base64,/.test(dataUrl) ? dataUrl : null;
  } catch (error: unknown) {
    console.error("Loading an audio file failed", error);
    return null;
  }
}

const cards = new Map<string, Promise<MumbleLinkStats | null>>();

/** The figures the engine's query answers for one link, asked once per link (issue #175). */
export function loadMumbleStats(url: string, refresh = false): Promise<MumbleLinkStats | null> {
  if (refresh) cards.delete(url);
  return memoized(cards, url, "the figures of a chat link", () =>
    getApi().mumbleLinkStats(url, refresh),
  );
}

/** Forgets loaded previews, images and query answers (tests). */
export function clearPreviews(): void {
  previews.clear();
  images.clear();
  audio.clear();
  cards.clear();
}

/**
 * How a card's player runs: Spotify's and SoundCloud's embeds directly in the app window (they
 * need no referrer, so they also run on Tauri's custom `tauri://` scheme); YouTube (also for
 * YouTube Music) and Twitch through the app's loopback player page (FR-2.41a), because YouTube
 * refuses to play without an http(s) referrer and Twitch only accepts an https or `localhost`
 * parent.
 */
export type CardPlayer = { via: "direct" | "page"; url: string };

function parseUrl(text: string): URL | null {
  try {
    return new URL(text);
  } catch {
    return null;
  }
}

/** The player of a video card, or `null` when the URL is not one of the known players. */
export function cardPlayer(video: VideoEmbed): CardPlayer | null {
  const url = parseUrl(video.playerUrl);
  return url !== null && PLAYER_ORIGINS.has(url.origin)
    ? { via: "page", url: url.toString() }
    : null;
}

/** The official embed of a music card, or `null` when it has none or the URL is not one of the
 * known players (issue #262). */
export function musicPlayer(music: MusicEmbed): CardPlayer | null {
  const url = music.playerUrl === null ? null : parseUrl(music.playerUrl);
  if (url === null) return null;
  const path = DIRECT_MUSIC_PLAYERS.get(url.origin);
  if (path !== undefined) {
    return url.pathname.startsWith(path) ? { via: "direct", url: url.toString() } : null;
  }
  return music.service === "youtubeMusic" && url.origin === "https://www.youtube-nocookie.com"
    ? { via: "page", url: url.toString() }
    : null;
}
