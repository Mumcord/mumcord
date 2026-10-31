/**
 * Which embeds of a message stay hidden because "Show link previews and embeds" is off
 * (issue #261). The kind is guessed from the address alone: finding out more would mean
 * contacting the linked site, which is exactly what the switch forbids.
 */
import { gifSource } from "./gifSource";
import { isImageLink } from "./links";

/** The embed types a hidden-embed hint can name. */
export type HiddenEmbedKind = "image" | "gif" | "video" | "audio" | "link";

function host(url: string): string {
  try {
    return new URL(url).hostname.toLowerCase().replace(/^www\./, "");
  } catch {
    return "";
  }
}

/** Hosts whose links play in a player card (mirrors the engine's `embed::recognise`). */
const VIDEO_HOSTS = new Set([
  "youtube.com",
  "m.youtube.com",
  "music.youtube.com",
  "youtu.be",
  "twitch.tv",
  "m.twitch.tv",
  "clips.twitch.tv",
]);

/** The kind of embed the link or `<img>` source would have shown with the switch on. */
export function hiddenEmbedKind(url: string, images: readonly string[]): HiddenEmbedKind {
  if (gifSource(url) !== null) return "gif";
  if (images.includes(url) || isImageLink(url)) {
    try {
      return new URL(url).pathname.toLowerCase().endsWith(".gif") ? "gif" : "image";
    } catch {
      return "image";
    }
  }
  const site = host(url);
  if (VIDEO_HOSTS.has(site)) return "video";
  if (site === "open.spotify.com") return "audio";
  return "link";
}
