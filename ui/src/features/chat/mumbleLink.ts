/**
 * The `mumble://` links of a message (issue #175). This is the single parsing rule for the UI and
 * the mock, and it matches `parse_mumble_link` in the engine: the link is untrusted input, so only
 * the scheme, the server address, the port, the channel path and the credentials are taken from it.
 * Credentials are never shown — what the card keeps is the fact that a link carries them.
 */
import type { TFunction } from "i18next";
import type { MumbleLink } from "../../api";

/** The parts of a saved server the card needs (issue #175). */
export type SavedServerRef = { id: number; label: string; address: string; port: number };

/** More cards than this per message would bury the conversation (the same cap as the previews). */
const MAX_LINKS = 3;

/** The port a Mumble server listens on when the link names none. */
export const DEFAULT_PORT = 64738;

/** Names are capped like the engine reads them; a longer name is not a channel. */
const MAX_SEGMENT = 64;

/** `null` for a link that is not a server link: no scheme, no address, or a port out of range. */
export function parseMumbleLink(url: string): MumbleLink | null {
  let link: URL;
  try {
    link = new URL(url);
  } catch {
    return null;
  }
  if (link.protocol !== "mumble:" || link.hostname === "") return null;
  const port = link.port === "" ? DEFAULT_PORT : Number(link.port);
  if (!Number.isInteger(port) || port < 1 || port > 65535) return null;
  return {
    address: link.hostname,
    port,
    username: link.username === "" ? null : decodeSegment(link.username),
    password: link.password === "" ? null : decodeSegment(link.password),
    channelPath: link.pathname
      .split("/")
      .filter((part) => part !== "")
      .map(decodeSegment),
  };
}

/** Whether a link carries credentials; only the fact is used, never the values. */
export function linkHasCredentials(url: string): boolean {
  const link = parseMumbleLink(url);
  return link !== null && (link.username !== null || link.password !== null);
}

/** The link without its credentials: what the card shows and what Copy link copies. */
export function canonicalLink(url: string): string {
  const link = parseMumbleLink(url);
  if (link === null) return url;
  const path = link.channelPath.map(encodeURIComponent).join("/");
  return `mumble://${link.address}:${String(link.port)}${path === "" ? "" : `/${path}`}`;
}

/**
 * The `mumble://` links of a message, in document order, at most three (issue #175). A link can
 * arrive as an anchor or as text the user typed, and a link without a server address gets no card.
 * The engine's own HTML is read here, because the UI sanitizer drops `mumble:` targets.
 */
export function mumbleLinks(html: string): string[] {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const links: string[] = [];
  const add = (url: string) => {
    if (parseMumbleLink(url) === null || links.length === MAX_LINKS || links.includes(url)) return;
    links.push(url);
  };
  const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    if (node instanceof HTMLAnchorElement) {
      const href = node.getAttribute("href")?.trim() ?? "";
      if (MUMBLE_HREF.test(href)) add(href);
    } else if (node.nodeType === Node.TEXT_NODE) {
      for (const match of (node.textContent ?? "").matchAll(MUMBLE_TEXT)) add(match[0]);
    }
  }
  return links;
}

/** How many links of a message carry credentials, for the warning's count (issue #175). */
export function credentialLinks(links: readonly string[]): number {
  return links.filter(linkHasCredentials).length;
}

/** The saved server a link points to, so the card names it as the server bar does (issue #175). */
export function savedServer(
  servers: readonly SavedServerRef[],
  link: MumbleLink,
): SavedServerRef | null {
  return servers.find((s) => s.address === link.address && s.port === link.port) ?? null;
}

/** Join, or what joining would do first: save the server, or open the channel of a connection. */
export function joinText(
  t: TFunction<"chat">,
  saved: SavedServerRef | null,
  connected: boolean,
): string {
  if (saved === null) return t("mumbleCard.addServer");
  if (connected) return t("mumbleCard.goToChannel");
  return t("mumbleCard.join");
}

const MUMBLE_HREF = /^mumble:\/\//i;
const MUMBLE_TEXT = /mumble:\/\/[^\s<>"']+/gi;

function decodeSegment(segment: string): string {
  try {
    return decodeURIComponent(segment).slice(0, MAX_SEGMENT);
  } catch {
    // A name that cannot be decoded is shown as it arrived.
    return segment.slice(0, MAX_SEGMENT);
  }
}
