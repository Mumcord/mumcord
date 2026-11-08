/**
 * Channel mentions (issue #643): `#Channelname` in a message links to a channel of the server
 * the message belongs to, and typing `#` in the message box suggests channels like `@` suggests
 * people. A channel mention is plain `#Name` text, so other Mumble clients just see that text.
 *
 * Names may contain spaces ("Raid night"): after `#` the longest channel name that matches wins,
 * and punctuation right after the name ends it.
 */
import type { ChannelInfo } from "../../api";
import { channelPath } from "../channelheader/channelModel";
import { foldName, MAX_SUGGESTIONS, type MentionEdit } from "./mentions";

export const CHANNEL_MENTION_CLASS = "chat-channel-mention";
/** Longest query that still counts as a channel name being typed. */
const MAX_QUERY = 40;

export interface ChannelQuery {
  /** Index of the `#` in the text. */
  start: number;
  /** What was typed after the `#`, up to the caret. */
  query: string;
}

/** The `#query` the caret is in, or null. The `#` must start the text or follow whitespace. */
export function findChannelQuery(text: string, caret: number): ChannelQuery | null {
  const before = text.slice(0, caret);
  const start = before.lastIndexOf("#");
  if (start < 0) return null;
  if (start > 0 && !/\s/u.test(before[start - 1] ?? "")) return null;
  const query = before.slice(start + 1);
  if (query.length > MAX_QUERY || /[\n\r]/u.test(query) || /^\s/u.test(query)) return null;
  return { start, query };
}

/** A channel as the picker and the message chips show it. */
export interface ChannelChoice {
  id: number;
  name: string;
  /** "Gaming › Raid" for nested channels; the name alone at the top level. */
  path: string;
}

/** The channels that can be mentioned: every channel but Root, which carries the server's name. */
export function mentionableChannels(channels: ReadonlyMap<number, ChannelInfo>): ChannelChoice[] {
  return [...channels.values()]
    .filter((c) => c.parentId !== null && c.name.trim() !== "")
    .map((c) => ({ id: c.id, name: c.name, path: channelPath(channels, c.id).join(" › ") }));
}

/**
 * Channels matching the query, case and diacritics ignored: names that start with it first, then
 * names that contain it, each group alphabetical.
 */
export function rankChannelCandidates(
  channels: readonly ChannelChoice[],
  query: string,
  limit = MAX_SUGGESTIONS,
): ChannelChoice[] {
  const wanted = foldName(query);
  const scored = channels
    .map((channel) => {
      const folded = foldName(channel.name);
      const rank = folded.startsWith(wanted) ? 0 : folded.includes(wanted) ? 1 : -1;
      return { channel, folded, rank };
    })
    .filter((c) => c.rank >= 0);
  scored.sort((a, b) =>
    a.rank !== b.rank
      ? a.rank - b.rank
      : a.folded < b.folded
        ? -1
        : a.folded > b.folded
          ? 1
          : a.channel.path.localeCompare(b.channel.path),
  );
  return scored.slice(0, limit).map((c) => c.channel);
}

/** Replaces `#query` (from `start` to `caret`) with `#Name ` and puts the caret behind it. */
export function insertChannelMention(
  text: string,
  start: number,
  caret: number,
  name: string,
): MentionEdit {
  const insert = `#${name} `;
  const after = text.slice(caret);
  const tail = after.startsWith(" ") ? after.slice(1) : after;
  return { text: text.slice(0, start) + insert + tail, caret: start + insert.length };
}

const WORD = /[\p{L}\p{N}_]/u;

/**
 * Turns every `#Name` of an existing channel in the text nodes of `html` into a chip. The longest
 * matching name wins; the name must not run on into a word (`#Raidnight` is not "Raid"), and the
 * `#` must start the text or follow something that is not a letter or digit (`a#b` is no mention).
 * Channels sharing a name share one chip, which lists them all.
 */
export function linkChannelMentions(
  html: string,
  channels: readonly ChannelChoice[],
  label: (name: string) => string,
): string {
  if (!html.includes("#") || channels.length === 0) return html;
  const byName = new Map<string, ChannelChoice[]>();
  for (const channel of channels) {
    const key = foldName(channel.name.trim());
    byName.set(key, [...(byName.get(key) ?? []), channel]);
  }
  // Longest first, so "Raid night" wins over "Raid".
  const names = [...byName.keys()].sort((a, b) => b.length - a.length);
  const doc = new DOMParser().parseFromString(html, "text/html");
  const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    // Text inside links, code and existing chips stays as it is.
    if (node instanceof Text && !node.parentElement?.closest("a, code, pre, .chat-mention")) {
      nodes.push(node);
    }
  }
  for (const node of nodes) {
    const source = node.data;
    if (!source.includes("#")) continue;
    // Folding keeps the length for nearly all text; where it does not, the chip is skipped.
    const folded = foldName(source);
    if (folded.length !== source.length) continue;
    const parts: (string | HTMLElement)[] = [];
    let copied = 0;
    for (let at = folded.indexOf("#"); at >= 0; at = folded.indexOf("#", at + 1)) {
      if (at < copied) continue;
      const before = folded[at - 1];
      if (before !== undefined && WORD.test(before)) continue;
      const name = names.find((n) => {
        if (!folded.startsWith(n, at + 1)) return false;
        const next = folded[at + 1 + n.length];
        return next === undefined || !WORD.test(next);
      });
      if (name === undefined) continue;
      const end = at + 1 + name.length;
      if (at > copied) parts.push(source.slice(copied, at));
      const matches = byName.get(name) ?? [];
      const chip = doc.createElement("span");
      chip.className = CHANNEL_MENTION_CLASS;
      chip.setAttribute("role", "button");
      chip.setAttribute("tabindex", "-1");
      chip.setAttribute("aria-haspopup", "menu");
      chip.setAttribute("aria-label", label(matches[0]?.name ?? name));
      chip.dataset["channelIds"] = matches.map((m) => String(m.id)).join(",");
      chip.textContent = source.slice(at, end);
      parts.push(chip);
      copied = end;
    }
    if (parts.length === 0) continue;
    if (copied < source.length) parts.push(source.slice(copied));
    node.replaceWith(...parts);
  }
  return doc.body.innerHTML;
}
