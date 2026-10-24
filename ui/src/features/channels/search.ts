/**
 * Channel tree search (QA.27): filters the tree to channels whose name matches and channels with
 * matching people. Matching ignores case and diacritics ("jose" finds "José") and looks for the
 * query anywhere in the name.
 */
import type { User } from "../../api";
import { channelRowId, memberRowId, type ChannelNode } from "./tree";

/** A name folded for matching, with the original position of every folded character. */
export interface FoldedName {
  text: string;
  /** `starts[i]` is where folded character `i` begins in the original name. */
  starts: number[];
  /** `ends[i]` is where folded character `i` ends in the original name. */
  ends: number[];
}

const MARKS = /\p{M}/gu;

/** Folds one string: lower case without diacritics. */
export function foldQuery(query: string): string {
  return query.normalize("NFD").replace(MARKS, "").toLowerCase().trim();
}

/** Folds a name character by character, keeping where each folded character came from. */
export function foldName(name: string): FoldedName {
  let text = "";
  const starts: number[] = [];
  const ends: number[] = [];
  let at = 0;
  for (const char of name) {
    const folded = char.normalize("NFD").replace(MARKS, "").toLowerCase();
    // A lone combining mark belongs to the character before it, also in a highlighted match.
    if (folded === "" && ends.length > 0) ends[ends.length - 1] = at + char.length;
    for (let i = 0; i < folded.length; i++) {
      starts.push(at);
      ends.push(at + char.length);
    }
    text += folded;
    at += char.length;
  }
  return { text, starts, ends };
}

/** Where `query` (already folded) occurs in `name`, as a range of the original name; or null. */
export function matchRange(name: FoldedName, query: string): { start: number; end: number } | null {
  if (query === "") return null;
  const index = name.text.indexOf(query);
  if (index < 0) return null;
  const start = name.starts[index];
  const end = name.ends[index + query.length - 1];
  return start === undefined || end === undefined ? null : { start, end };
}

/** Folded names of all channels and people in the tree, built once per tree. */
export interface SearchIndex {
  channels: Map<number, string>;
  people: Map<number, string>;
}

export function buildSearchIndex(roots: readonly ChannelNode[]): SearchIndex {
  const channels = new Map<number, string>();
  const people = new Map<number, string>();
  const walk = (node: ChannelNode) => {
    channels.set(node.channel.id, foldName(node.channel.name).text);
    for (const user of node.members) people.set(user.session, foldName(user.name).text);
    node.children.forEach(walk);
  };
  roots.forEach(walk);
  return { channels, people };
}

export interface SearchResult {
  /**
   * The filtered tree. Channels list only their matching people (`shown`), even when they are
   * crowded; nodes keep the channel's full `members` for its people count.
   */
  roots: ChannelNode[];
  /** Channels whose name matches. */
  channels: number;
  /** People whose name matches. */
  people: number;
  /** Row ids of the matches in display order (matching channel, then its matching people). */
  matches: string[];
}

/**
 * Filters `roots` by `query` (already folded, not empty). With the `previous` result, every node
 * that would come out the same keeps its object, so its memoised row skips re-rendering while
 * people move during a search.
 */
export function searchTree(
  roots: readonly ChannelNode[],
  index: SearchIndex,
  query: string,
  previous?: SearchResult,
): SearchResult {
  const before = new Map<number, ChannelNode>();
  const remember = (node: ChannelNode) => {
    before.set(node.channel.id, node);
    node.children.forEach(remember);
  };
  previous?.roots.forEach(remember);

  let channels = 0;
  let people = 0;
  const matches: string[] = [];
  const filter = (node: ChannelNode): ChannelNode | null => {
    const nameMatches = index.channels.get(node.channel.id)?.includes(query) ?? false;
    const found: User[] = node.members.filter(
      (user) => index.people.get(user.session)?.includes(query) ?? false,
    );
    // A matching channel without matching people shows its people as usual; any other channel
    // lists only its matching people (none for channels on the path to a match).
    const shown = nameMatches && found.length === 0 ? undefined : found;
    const at = matches.length;
    if (nameMatches) channels++;
    people += found.length;
    const children = node.children.flatMap((child) => filter(child) ?? []);
    if (!nameMatches && found.length === 0 && children.length === 0) return null;
    // Matches in display order: this row and its people come before its sub-channels' rows.
    matches.splice(
      at,
      0,
      ...(nameMatches ? [channelRowId(node.channel.id)] : []),
      ...found.map((user) => memberRowId(user.session)),
    );
    const old = before.get(node.channel.id);
    if (
      old !== undefined &&
      old.channel === node.channel &&
      old.members === node.members &&
      (old.shown === undefined || shown === undefined
        ? old.shown === shown
        : sameItems(old.shown, shown)) &&
      sameItems(old.children, children)
    ) {
      return old;
    }
    return { channel: node.channel, members: node.members, children, shown };
  };
  return { roots: roots.flatMap((root) => filter(root) ?? []), channels, people, matches };
}

function sameItems<T>(a: readonly T[], b: readonly T[]): boolean {
  return a.length === b.length && a.every((item, i) => item === b[i]);
}
