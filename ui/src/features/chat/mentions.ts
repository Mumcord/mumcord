/**
 * Mentions (FR-1.51): finding the `@` query at the caret, ranking people for the autocomplete,
 * inserting the chosen name, and detecting whether a received message mentions the user.
 * A mention is plain `@Name` text, so other Mumble clients just see that text.
 */
import type { User } from "../../api";

/** DOM id of option `index` of the suggestion list `listId`. */
export function optionId(listId: string, index: number): string {
  return `${listId}-${String(index)}`;
}

export const MAX_SUGGESTIONS = 8;
/** Longest query that still counts as a name being typed (names may contain spaces). */
const MAX_QUERY = 40;

/** Lower-cases and strips diacritics so "jose" finds "José". */
export function foldName(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

export interface MentionQuery {
  /** Index of the `@` in the text. */
  start: number;
  /** What was typed after the `@`, up to the caret. */
  query: string;
}

/** The `@query` the caret is in, or null. The `@` must start the text or follow whitespace. */
export function findMentionQuery(text: string, caret: number): MentionQuery | null {
  const before = text.slice(0, caret);
  const start = before.lastIndexOf("@");
  if (start < 0) return null;
  if (start > 0 && !/\s/u.test(before[start - 1] ?? "")) return null;
  const query = before.slice(start + 1);
  if (query.length > MAX_QUERY || /[\n\r]/u.test(query) || /^\s/u.test(query)) return null;
  return { start, query };
}

/**
 * People matching the query by name prefix (case and diacritics ignored): people in
 * `channelId` first, then the others, each group alphabetical. The user themself is left out.
 */
export function rankMentionCandidates(
  users: Iterable<User>,
  query: string,
  channelId: number | null,
  selfSession: number | null,
  limit = MAX_SUGGESTIONS,
): User[] {
  const wanted = foldName(query);
  const matches: { user: User; folded: string; here: boolean }[] = [];
  for (const user of users) {
    if (user.session === selfSession || user.name === "") continue;
    const folded = foldName(user.name);
    if (folded.startsWith(wanted)) {
      matches.push({ user, folded, here: channelId !== null && user.channelId === channelId });
    }
  }
  matches.sort((a, b) => {
    if (a.here !== b.here) return a.here ? -1 : 1;
    return a.folded < b.folded ? -1 : a.folded > b.folded ? 1 : a.user.session - b.user.session;
  });
  return matches.slice(0, limit).map((m) => m.user);
}

export interface MentionEdit {
  text: string;
  caret: number;
}

/** Replaces `@query` (from `start` to `caret`) with `@Name ` and puts the caret behind it. */
export function insertMention(
  text: string,
  start: number,
  caret: number,
  name: string,
): MentionEdit {
  const insert = `@${name} `;
  const after = text.slice(caret);
  // Avoid a double space when the text already continues with one.
  const tail = after.startsWith(" ") ? after.slice(1) : after;
  return { text: text.slice(0, start) + insert + tail, caret: start + insert.length };
}

const WORD = /[\p{L}\p{N}_]/u;

/**
 * True when `text` contains `@name` as a whole word: not glued to a preceding word (an email
 * address is no mention) and not continuing into a longer name. Case and diacritics are ignored.
 */
export function mentionsName(text: string, name: string): boolean {
  const needle = `@${foldName(name.trim())}`;
  if (needle.length < 2) return false;
  const haystack = foldName(text);
  for (let from = haystack.indexOf(needle); from >= 0; from = haystack.indexOf(needle, from + 1)) {
    const before = haystack[from - 1];
    if (before !== undefined && WORD.test(before)) continue;
    const end = from + needle.length;
    const next = haystack[end];
    if (next === undefined) return true;
    if (WORD.test(next)) continue;
    // "@Bob.com" is a web name, "@Bob." at a sentence end is a mention.
    const afterNext = haystack[end + 1];
    if ((next === "." || next === "-") && afterNext !== undefined && WORD.test(afterNext)) continue;
    return true;
  }
  return false;
}

const BLOCKS = "br, p, div, li, tr, h1, h2, h3, h4, h5, h6, blockquote, pre";

/** Plain text of a sanitized message body; parsing into an inert document never loads anything. */
export function bodyText(html: string): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  for (const block of doc.body.querySelectorAll(BLOCKS)) block.after(" ");
  return doc.body.textContent;
}

/** True when the message body mentions any of `names`. */
export function bodyMentions(html: string, names: readonly string[]): boolean {
  if (!html.includes("@")) return false;
  const text = bodyText(html);
  return names.some((name) => mentionsName(text, name));
}
