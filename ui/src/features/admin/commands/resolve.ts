/**
 * Finding the person or channel an admin typed (Epic 4, issue #378, FR-10.34): one match runs the
 * command, several ask which one was meant, and none gives a hint with the closest name. Pure
 * functions over the server state, so the rules are testable without a connection.
 */
import type { ChannelInfo, User } from "../../../api";

/** Something an argument can name: a person, a channel or a ban, with the hint shown beside it. */
export interface Candidate<T> {
  readonly name: string;
  /** What shows as the hint: the channel a person is in, a channel's parent, a ban's reason. */
  readonly hint: string;
  readonly item: T;
}

/** What a typed argument names. */
export type Resolved<T> =
  | { kind: "one"; item: T }
  /** Several match; the admin picks one (mockup `04c-error-ambiguous`). */
  | { kind: "many"; items: readonly Candidate<T>[] }
  /** Nothing matches; `suggestion` is the closest name, when there is one (mockup `04b`). */
  | { kind: "none"; suggestion: string | null };

/**
 * A leading `@` is a mention; a command takes the bare name, so `/kick @Mallory` works exactly
 * like `/kick Mallory`. This is the same the mention picker does, so a name picked there fits.
 */
function bare(query: string): string {
  return query.startsWith("@") ? query.slice(1) : query;
}

/** A word is a full match when a name equals it, a start match when it begins it, else a part match. */
function rank(query: string, name: string): number {
  const lowered = name.toLowerCase();
  const wanted = bare(query).toLowerCase();
  if (lowered === wanted) return 0;
  if (lowered.startsWith(wanted)) return 1;
  if (lowered.includes(wanted)) return 2;
  return -1;
}

/**
 * The candidates a word matches, best first. Names compare without case, and only the front of a
 * name counts as a start match: "Mall" finds Mallory, while "ory" only finds her as a part match.
 */
export function ranked<T>(query: string, items: readonly Candidate<T>[]): Candidate<T>[] {
  if (query === "") return [...items];
  return items
    .map((item) => ({ item, score: rank(query, item.name) }))
    .filter((entry) => entry.score >= 0)
    .sort((a, b) => a.score - b.score || a.item.name.localeCompare(b.item.name))
    .map((entry) => entry.item);
}
/** Resolves a word to one item, asks which of several was meant, or finds nothing. */
export function resolveOne<T>(
  query: string,
  items: readonly Candidate<T>[],
  /** Whether a single partial match counts as the answer; only safe where a confirmation follows. */
  partialRuns = false,
): Resolved<T> {
  const matches = ranked(query, items);
  const wanted = bare(query).toLowerCase();
  const exact = matches.filter((item) => item.name.toLowerCase() === wanted);
  // A full name wins over people who only contain it: `/mute Mall` means Mallory, not Mallory Ann.
  if (exact.length === 1) {
    const only = exact[0];
    if (only !== undefined) return { kind: "one", item: only.item };
  }
  if (exact.length > 1) return { kind: "many", items: exact };
  // A part of a name is a guess, and a guess must not mute, move or register someone: the admin
  // picks it from the list instead.
  const onlyMatch = matches[0];
  if (partialRuns && matches.length === 1 && onlyMatch !== undefined) {
    return { kind: "one", item: onlyMatch.item };
  }
  if (matches.length > 0) return { kind: "many", items: matches };
  return {
    kind: "none",
    suggestion: closest(
      query,
      items.map((item) => item.name),
    ),
  };
}

/**
 * Resolves the start of an argument that may hold spaces ("Bob Smith spamming"): the longest run
 * of words that is a full name wins, so "Bob Smith" is never read as "Bob" plus a reason. Without
 * a full name the first word is resolved alone, which can only ask or fail. `used` is how many
 * words the answer consumed; the rest belong to the command.
 */
export function resolveLeading<T>(
  words: readonly string[],
  items: readonly Candidate<T>[],
): { found: Resolved<T>; used: number } {
  for (let count = words.length; count >= 1; count -= 1) {
    const found = resolveOne(words.slice(0, count).join(" "), items);
    const full = found.kind === "one" || (found.kind === "many" && isFull(words, count, found));
    if (full) return { found, used: count };
  }
  return { found: resolveOne(words[0] ?? "", items), used: 1 };
}

/** Whether a many-result is a set of exact duplicates rather than partial matches. */
function isFull<T>(words: readonly string[], count: number, found: Resolved<T>): boolean {
  if (found.kind !== "many") return false;
  const wanted = bare(words.slice(0, count).join(" ")).toLowerCase();
  return found.items.every((item) => item.name.toLowerCase() === wanted);
}

/**
 * The name closest to a misspelling (mockup `04b-error-unknown-user`: "Did you mean Mallory?").
 * Small edits only, so a word nobody resembles gives no hint.
 */
export function closest(query: string, names: readonly string[]): string | null {
  const wanted = bare(query).toLowerCase();
  const budget = Math.max(1, Math.floor(wanted.length / 3));
  let best: { name: string; distance: number } | null = null;
  for (const name of names) {
    if (name.toLowerCase() === wanted) continue;
    const distance = editDistance(wanted, name.toLowerCase());
    if (distance <= budget && (best === null || distance < best.distance)) {
      best = { name, distance };
    }
  }
  return best?.name ?? null;
}

/** Levenshtein distance, with only two rows kept because names are short. */
export function editDistance(a: string, b: string): number {
  if (a === b) return 0;
  if (a === "") return b.length;
  if (b === "") return a.length;
  let previous = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 1; i <= a.length; i += 1) {
    const current = [i];
    for (let j = 1; j <= b.length; j += 1) {
      current[j] = Math.min(
        (previous[j] ?? 0) + 1,
        (current[j - 1] ?? 0) + 1,
        (previous[j - 1] ?? 0) + (a[i - 1] === b[j - 1] ? 0 : 1),
      );
    }
    previous = current;
  }
  return previous[b.length] ?? 0;
}

/** The people an argument may name, with their channel as the hint. */
export function personCandidates(
  users: Iterable<User>,
  self: number | null,
  may: (user: User) => boolean,
  channelName: (id: number) => string,
): Candidate<User>[] {
  const out: Candidate<User>[] = [];
  for (const user of users) {
    if (self !== null && user.session === self) continue;
    if (!may(user)) continue;
    out.push({ name: user.name, hint: channelName(user.channelId), item: user });
  }
  return out;
}

/** The channels an argument may name, with their parent as the hint. */
export function channelCandidates(
  entries: Iterable<ChannelInfo>,
  parentName: (parentId: number | null) => string,
): Candidate<ChannelInfo>[] {
  return [...entries].map((channel) => ({
    name: channel.name,
    hint: parentName(channel.parentId),
    item: channel,
  }));
}
