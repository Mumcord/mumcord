/**
 * YouTube videos of the watch party (FR-2.39, FR-2.41). Only YouTube is allowed: video ids are
 * checked before they reach the player page (see `playerBridge.ts`), which builds the one fixed
 * embed URL.
 */

/** The origin of the YouTube embeds the engine delivers in video cards. */
export const YOUTUBE_ORIGIN = "https://www.youtube-nocookie.com";

const VIDEO_ID = /^[A-Za-z0-9_-]{11}$/;

export function isVideoId(id: string): boolean {
  return VIDEO_ID.test(id);
}

/** The video id of a YouTube embed URL as the engine delivers it in a video card, or `null`. */
export function videoIdOf(playerUrl: string): string | null {
  let url: URL;
  try {
    url = new URL(playerUrl);
  } catch {
    return null;
  }
  if (url.origin !== YOUTUBE_ORIGIN) return null;
  const match = /^\/embed\/([^/]+)$/.exec(url.pathname);
  const id = match?.[1] ?? "";
  return isVideoId(id) ? id : null;
}

/** The video on youtube.com, for "Open in browser". */
export function watchUrl(videoId: string): string {
  return `https://www.youtube.com/watch?v=${encodeURIComponent(videoId)}`;
}

/** YouTube player states as the control protocol reports them (via the player page). */
export const ENDED = 0;
export const PLAYING = 1;
export const PAUSED = 2;
/** Buffering: the player is waiting for data, so it is behind the party. */
export const BUFFERING = 3;

const LINK_HOSTS = new Set([
  "youtube.com",
  "www.youtube.com",
  "m.youtube.com",
  "music.youtube.com",
]);

/**
 * The video id of a pasted YouTube link (watch, youtu.be, shorts, embed or live form), or `null`
 * for anything else, so only real YouTube videos can start a party (FR-2.65).
 */
export function videoIdOfLink(text: string): string | null {
  let url: URL;
  try {
    url = new URL(text.trim());
  } catch {
    return null;
  }
  if (url.protocol !== "https:" && url.protocol !== "http:") return null;
  let id: string | null | undefined = null;
  if (url.hostname === "youtu.be") {
    id = url.pathname.split("/")[1];
  } else if (LINK_HOSTS.has(url.hostname)) {
    id =
      url.pathname === "/watch"
        ? url.searchParams.get("v")
        : /^\/(?:shorts|embed|live)\/([^/]+)$/.exec(url.pathname)?.[1];
  }
  return id !== null && id !== undefined && isVideoId(id) ? id : null;
}
