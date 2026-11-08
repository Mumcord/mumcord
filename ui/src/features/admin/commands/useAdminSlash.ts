/**
 * The built-in admin commands of the chat box (Epic 4, issue #378). This hook is the part the
 * composer needs: what the list offers while typing (commands the server's rights allow, then the
 * people or channels an argument may name — FR-10.32), and what a complete line does. Running it
 * is in `run.ts`; the rights and the argument shapes are in `commands.ts`.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type BanEntry, type Conversation } from "../../../api";
import type { BuiltinSlash, SlashOption } from "../../chat/slashCommands";
import { useServerState } from "../../channels/serverStore";
import { currentBanWindow } from "../banStore";
import { adminCommand, allowedCommands } from "./commands";
import { commandName, slashQuery } from "./parse";
import { channelCandidates, ranked, type Candidate } from "./resolve";
import { isBuiltIn, peopleOf, runBuiltIn } from "./run";
import type { CommandContext } from "./context";

/** How many rows an argument offers at once. */
const MAX_ROWS = 8;

export interface AdminSlash extends BuiltinSlash {
  /** `null` when the composer is not on a synced server, so no command could run. */
  readonly serverId: number | null;
}

/**
 * The admin commands for the chat box that is shown. `conversation` is where the result lines of
 * a command land, which is the chat the command was typed in (FR-10.35, Q4.6).
 */
export function useAdminSlash(
  text: string,
  setText: (text: string) => void,
  disabled: boolean,
  conversation: Conversation,
): AdminSlash {
  const { t } = useTranslation("admin");
  const state = useServerState();
  const serverId = state.serverId;
  const [notice, setNotice] = useState<{ line: string; text: string } | null>(null);
  const [choice, setChoice] = useState<{ line: string; options: SlashOption[] } | null>(null);
  const [bans, setBans] = useState<{ serverId: number; entries: readonly BanEntry[] } | null>(null);
  const query = disabled ? null : slashQuery(text);
  // A command takes the line out of the box, so a hint or a choice list belongs to the empty box
  // that is left behind: the next word typed makes it meaningless (FR-10.34).
  const shownNotice = notice?.line === text ? notice.text : null;
  const shownChoice = choice?.line === text ? choice.options : null;

  // `/unban` names an entry of the server's list, so the list is read while that command is being
  // typed and dropped again afterwards (FR-10.32, FR-10.63).
  const readingBans = !disabled && query?.name === "unban" && serverId !== null;
  useEffect(() => {
    if (!readingBans) return;
    let open = true;
    getApi()
      .banListOpen(serverId)
      .then(
        (entries) => {
          if (open) setBans({ serverId, entries });
        },
        () => {
          // A server that will not show its list simply offers nothing; running `/unban` says why.
          if (open) setBans(null);
        },
      );
    return () => {
      open = false;
      // An open administration window keeps its own copy; its data must not go with the command.
      if (currentBanWindow()?.serverId !== serverId) void getApi().banListClose(serverId);
    };
  }, [readingBans, serverId]);

  const options = useMemo<SlashOption[]>(() => {
    if (shownChoice !== null) return shownChoice;
    if (disabled || query === null || serverId === null) return [];
    if (query.naming) {
      return allowedCommands(state)
        .filter((command) => command.name.startsWith(query.name))
        .map((command) => ({
          id: `builtin-${command.name}`,
          group: t("commands.group"),
          keyword: `/${command.name}`,
          hint: t(`commands.usage.${command.name}`),
          tag: t("commands.builtIn"),
          insert: `/${command.name} `,
        }));
    }
    const command = adminCommand(query.name);
    // A command the admin may not use is not there at all, so it is not autocompleted either.
    if (command === undefined || !command.allowed(state)) return [];
    // Only the list of the server this command is typed on counts, so a stale read is never shown.
    const knownBans = bans !== null && bans.serverId === serverId ? bans.entries : [];
    const kind = command.args[query.words.length];
    if (kind === undefined) return [];
    const candidates: Candidate<unknown>[] =
      kind === "person"
        ? peopleOf(command, state)
        : kind === "channel"
          ? channelCandidates(state.channels.values(), (parentId) =>
              parentId === null ? "" : (state.channels.get(parentId)?.name ?? ""),
            )
          : knownBans.map((entry) => ({
              name: entry.name ?? entry.address,
              hint: entry.address,
              item: entry,
            }));
    return ranked(query.partial, candidates)
      .slice(0, MAX_ROWS)
      .map((candidate, index) => ({
        id: `${kind}-${String(index)}-${candidate.name}`,
        group: argumentHeading(kind, t),
        keyword: candidate.name,
        hint: candidate.hint,
        tag: "",
        insert: replaceLastWord(text, candidate.name),
      }));
  }, [shownChoice, disabled, query, serverId, state, bans, t, text]);

  const run = useCallback(
    (line: string): boolean => {
      if (disabled || serverId === null || !isBuiltIn(line)) return false;
      const name = commandName(line);
      const command = name === null ? undefined : adminCommand(name);
      // A command without its right does nothing at all and leaves the text in the box (FR-10.33).
      if (command === undefined || !command.allowed(state)) return true;
      setChoice(null);
      setNotice(null);
      setText("");
      const ctx: CommandContext = { serverId, conversation, state, t };
      void runBuiltIn(line, ctx).then((result) => {
        // The line is out of the box by now, so both belong to the empty box (FR-10.34).
        if (result.kind === "notice") setNotice({ line: "", text: result.text });
        else if (result.kind === "choice") {
          // Choosing one of these runs the command, so the list closes with the line.
          setChoice({
            line: "",
            options: result.options.map((option) => ({
              ...option,
              run: () => {
                setChoice(null);
                option.run?.();
              },
            })),
          });
        }
      });
      return true;
    },
    [disabled, serverId, state, conversation, setText, t],
  );

  return { options, run, notice: shownNotice, serverId };
}

/** The rows of an argument list say what kind of thing they are (mockup `02-name-autocomplete`). */
function argumentHeading(kind: string, t: (key: string) => string): string {
  switch (kind) {
    case "channel":
      return t("commands.groupChannels");
    case "ban":
      return t("commands.groupBans");
    default:
      return t("commands.groupPeople");
  }
}

/** What the box holds after an argument word was completed: the word, then a space. */
export function replaceLastWord(text: string, word: string): string {
  const match = /^(.*?[ \t]+)?([^ \t]*)$/s.exec(text);
  const head = match?.[1] ?? "";
  return `${head}${word} `;
}
