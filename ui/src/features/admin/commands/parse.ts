/**
 * Splitting a typed admin slash command into its command name and its argument words (Epic 4,
 * issue #378, FR-10.30). Pure text work: what the words mean and who they name is decided in
 * `resolve.ts`, and what is offered while typing in `commands.ts`.
 */

/** A typed line that is a command at all, with the word the caret is still writing. */
export interface SlashQuery {
  /** The command name as typed, lower case (`"kick"` for `/kick Ma`). */
  readonly name: string;
  /** The argument words that are finished, in order (`["Freya"]` for `/move Freya A`). */
  readonly words: readonly string[];
  /** The last word, still being typed; `""` right after the space of `/kick `. */
  readonly partial: string;
  /** True while only the command name has been typed (`/`, `/kic`): the list shows commands. */
  readonly naming: boolean;
}

/**
 * The command a line holds, or `null` when it is plain text. A command name is letters, digits
 * and `-` (the same shape plugin commands use), and `/` alone counts as a name being written, so
 * the bare slash opens the list (mockup `01-command-list`). The arguments keep their spaces: the
 * reason of `/kick Mallory spam links` is one text, not three words.
 */
export function slashQuery(text: string): SlashQuery | null {
  const match = /^\/([a-z0-9-]*)([ \t].*)?$/s.exec(text);
  if (match === null) return null;
  const name = match[1] ?? "";
  const tail = match[2] ?? "";
  if (tail === "") return { name, words: [], partial: "", naming: true };
  // Only the last word is still being written; earlier ones are complete arguments. An argument
  // that is finished shows up as an empty word here, so it is dropped rather than counted.
  const rest = tail.slice(1);
  const endsWithSpace = /[ \t]$/.test(rest);
  const tokens = rest.split(/[ \t]+/);
  const last = (tokens.pop() ?? "").trim();
  return {
    name,
    words: tokens.filter((token) => token !== ""),
    partial: endsWithSpace ? "" : last,
    naming: false,
  };
}

/** Whether a typed line starts with a command name at all (`/ban …` → true). */
export function commandName(text: string): string | null {
  const match = /^\/([a-z0-9-]+)(?:[ \t]|$)/.exec(text);
  return match?.[1] ?? null;
}

/**
 * The `[duration]` token of `/ban <name> [duration] [reason]` (FR-10.31): `<n>h` and `<n>d` are
 * hours and days, `perm` is permanent. `null` when the word is no duration, which means it is
 * already the reason.
 */
export function parseDuration(word: string | undefined): number | null {
  if (word === undefined) return null;
  const lowered = word.toLowerCase();
  if (lowered === "perm" || lowered === "permanent") return 0;
  const match = /^(\d+)([hd])$/.exec(lowered);
  if (match === null) return null;
  const value = Number(match[1]);
  if (!Number.isFinite(value) || value <= 0) return null;
  return match[2] === "h" ? value * 3600 : value * 86_400;
}
