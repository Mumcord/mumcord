/**
 * The command list of the chat box (Epic 4, issues #378 and #166/FR-6.43): Mumcord's own admin
 * commands first, then the running plugins' commands, in one list the keyboard moves through.
 * Choosing a row writes it into the box; Enter runs a complete line. A command's text never goes
 * into the chat, and one the admin may not use behaves like a command that does not exist:
 * nothing is sent and the text stays (FR-10.33).
 */
import { useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { commandOf, matchCommands, usePluginChat } from "../plugins/chat/pluginChatStore";
import { getShownServerId } from "../channels/serverStore";

/** One row of the list: a command, or a person or channel an argument names. */
export interface SlashOption {
  readonly id: string;
  /** Section the row sits in, as its own heading in the list. */
  readonly group: string;
  /** What shows first: `/kick`, or the name of the person or channel. */
  readonly keyword: string;
  /** Usage hint, or the channel a person is in. */
  readonly hint: string;
  /** At the end of the row: "Built-in", or the plugin's name. */
  readonly tag: string;
  /** The whole content the box gets when the row is chosen. */
  readonly insert: string;
  /** Runs the line at once instead of only completing it — an argument whose name was unclear. */
  readonly run?: () => void;
}

/**
 * What Mumcord's own commands bring to the box. They are collected in the admin feature, which
 * owns their rights and their actions, and only cross here as options and a way to run them.
 */
export interface BuiltinSlash {
  readonly options: readonly SlashOption[];
  /** True when the line is one of Mumcord's commands, run or not: it never reaches the chat. */
  run: (line: string) => boolean;
  /** A line that could not be run, shown above the box; `null` when there is nothing to say. */
  readonly notice: string | null;
}

export interface SlashBox {
  open: boolean;
  options: readonly SlashOption[];
  active: number;
  pick: (option: SlashOption) => void;
  /** Handles the keys of the open list; true when the key was used. */
  onKey: (event: KeyboardEvent<HTMLTextAreaElement>) => boolean;
  /**
   * Enter on a complete line: runs it and empties the box. False when the line is no command at
   * all, which means the caller sends it to the chat as usual.
   */
  run: (line: string) => boolean;
  /** The reason a line did not run, already translated; `null` when there is none. */
  error: string | null;
}

export function commandOptionId(listId: string, index: number): string {
  return `${listId}-command-${String(index)}`;
}

/** The plugin commands typed so far: `/`, `/kic`, but not `/kick Mallory`. */
function pluginQuery(text: string): string | null {
  const match = /^\/([a-z0-9-]*)$/.exec(text);
  return match?.[1] ?? null;
}

export function useSlashCommands(
  text: string,
  setText: (text: string) => void,
  disabled: boolean,
  builtin?: BuiltinSlash,
): SlashBox {
  const { t } = useTranslation("pluginChat");
  const { commands } = usePluginChat();
  // The highlighted row belongs to the query it was chosen for; a new query starts at the top.
  const [selected, setSelected] = useState<{ query: string | null; index: number }>({
    query: null,
    index: 0,
  });
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const query = disabled ? null : pluginQuery(text);
  const pluginOptions: SlashOption[] =
    query === null
      ? []
      : matchCommands(commands, query).map((command) => ({
          id: `${command.pluginId}:${command.name}`,
          group: t("command.group"),
          keyword: `/${command.name}`,
          hint: command.description + (command.usage === null ? "" : ` · ${command.usage}`),
          tag: command.pluginName,
          insert: `/${command.name} `,
        }));
  // Mumcord's own commands come first, and an argument they are asking for replaces them (the
  // built-in hook only ever offers one kind at a time).
  const options = disabled ? [] : [...(builtin?.options ?? []), ...pluginOptions];
  const open = options.length > 0 && dismissed !== text;

  const active = selected.query === text ? selected.index : 0;
  const index = Math.min(active, Math.max(options.length - 1, 0));
  const setActive = (next: number) => {
    setSelected({ query: text, index: next });
  };

  const choose = (option: SlashOption) => {
    setDismissed(null);
    if (option.run !== undefined) {
      option.run();
      return;
    }
    setText(option.insert);
  };

  const onKey = (event: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    if (!open || event.nativeEvent.isComposing) return false;
    const chosen = options[index];
    switch (event.key) {
      case "ArrowDown":
        setActive((index + 1) % options.length);
        break;
      case "ArrowUp":
        setActive((index - 1 + options.length) % options.length);
        break;
      case "Tab":
        if (chosen === undefined) return false;
        choose(chosen);
        break;
      case "Enter":
        // A row that still needs completing is completed first; one that runs, runs.
        if (chosen === undefined || event.shiftKey) return false;
        if (chosen.run !== undefined) {
          choose(chosen);
          break;
        }
        if (!isCompletion(chosen, text)) return false;
        choose(chosen);
        break;
      case "Escape":
        setDismissed(text);
        break;
      default:
        return false;
    }
    event.preventDefault();
    return true;
  };

  const run = (line: string): boolean => {
    if (disabled) return false;
    if (builtin?.run(line) === true) {
      setError(builtin.notice ?? null);
      return true;
    }
    if (commandOf(commands, line) === undefined) return false;
    setText("");
    setError(null);
    getApi()
      // The chat box belongs to the chat on screen: a reply goes to that server.
      .pluginRunCommand(line, getShownServerId())
      .then(
        (outcome) => {
          // The plugin stopped in the meantime: give the line back rather than lose it.
          if (outcome === "unknown") {
            setText(line);
            setError(t("command.unavailable"));
          }
        },
        (failure: unknown) => {
          console.error("Running a plugin command failed", failure);
          setText(line);
          setError(t("command.failed"));
        },
      );
    return true;
  };

  return { open, options, active: index, pick: choose, onKey, run, error: error ?? null };
}

/**
 * Whether a row really completes what is typed: a command name that is still missing letters, or
 * an argument word the box does not hold yet. Fully typed lines pass, so Enter runs them.
 */
function isCompletion(option: SlashOption, text: string): boolean {
  if (option.keyword.startsWith("/")) {
    // `/kick ` still names nothing: the row is `/kick`, and it is longer than what is typed.
    return option.keyword.length > text.length && option.keyword.startsWith(text);
  }
  // An argument row writes the word the box is still missing, so it is always a completion.
  return !text.endsWith(`${option.keyword} `) && !text.endsWith(option.keyword);
}
