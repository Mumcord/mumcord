/**
 * The canonical video URL of a chat link (issue #419): what "Copy link" puts on the clipboard, so
 * a copied embed carries the address of the video itself and none of the parameters the players
 * add (`parent`, `si`, `t`, autoplay). Only the two card services have a canonical form; every
 * other link is copied exactly as the message wrote it.
 */

const YOUTUBE_ID = /^[A-Za-z0-9_-]{11}$/;
const TWITCH_CLIP_ID = /^[A-Za-z0-9]{15,50}$/;
const TWITCH_CHANNEL = /^[A-Za-z0-9_]{1,50}$/;

const YOUTUBE_HOSTS = new Set([
  "youtube.com",
  "www.youtube.com",
  "m.youtube.com",
  "youtu.be",
  "www.youtube-nocookie.com",
]);

function parse(url: string): URL | null {
  try {
    return new URL(url);
  } catch {
    return null;
  }
}

/** The YouTube video id of a watch, embed, shorts or `youtu.be` address, or `null` for a page. */
function youTubeId(parsed: URL): string | null {
  const match =
    /^\/embed\/([^/]+)$/.exec(parsed.pathname) ??
    /^\/shorts\/([^/]+)$/.exec(parsed.pathname) ??
    /^\/watch$/.exec(parsed.pathname) ??
    (parsed.hostname === "youtu.be" ? /^\/([^/]+)$/.exec(parsed.pathname) : null);
  const id = match?.[1] ?? parsed.searchParams.get("v") ?? "";
  return YOUTUBE_ID.test(id) ? id : null;
}

/** The canonical address of a YouTube or Twitch link, or the link itself for anything else. */
export function canonicalVideoUrl(url: string): string {
  const parsed = parse(url);
  if (parsed === null) return url;
  const host = parsed.hostname;

  if (YOUTUBE_HOSTS.has(host)) {
    const id = youTubeId(parsed);
    if (id === null) return url;
    // A short stays a short: its watch page is not the same address.
    return parsed.pathname.startsWith("/shorts/")
      ? `https://www.youtube.com/shorts/${id}`
      : `https://www.youtube.com/watch?v=${id}`;
  }

  if (host === "player.twitch.tv") {
    const channel = parsed.searchParams.get("channel") ?? "";
    return TWITCH_CHANNEL.test(channel) ? `https://www.twitch.tv/${channel}` : url;
  }
  if (host === "clips.twitch.tv") {
    const clip =
      parsed.searchParams.get("clip") ?? /^\/embed\/([^/]+)$/.exec(parsed.pathname)?.[1] ?? "";
    return TWITCH_CLIP_ID.test(clip) ? `https://clips.twitch.tv/${clip}` : url;
  }

  return url;
}
