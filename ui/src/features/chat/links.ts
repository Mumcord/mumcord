/**
 * Which links of a message get a preview card, the shared preview lookup and the rules for
 * embedding video players (FR-1.49, FR-1.50).
 */
import { getApi, type LinkPreview, type VideoEmbed } from "../../api";

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

const previews = new Map<string, Promise<LinkPreview | null>>();

/** The engine's preview of `url`, requested once per app run (the engine caches across runs). */
export function loadPreview(url: string): Promise<LinkPreview | null> {
  let preview = previews.get(url);
  if (!preview) {
    preview = getApi()
      .linkPreview(url)
      .catch((error: unknown) => {
        console.error("Loading a link preview failed", error);
        previews.delete(url);
        return null;
      });
    previews.set(url, preview);
  }
  return preview;
}

/** Forgets loaded previews (tests). */
export function clearPreviews(): void {
  previews.clear();
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
