/**
 * Slash commands of plugins in the chat box (FR-6.43): which commands match what is typed, and a
 * typed command goes to its plugin instead of into the chat.
 */
import { useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type PluginCommand } from "../../../api";
import { commandOf, matchCommands, usePluginChat } from "./pluginChatStore";
import { getShownServerId } from "../../channels/serverStore";

export function commandQuery(text: string): string | null {
  const match = /^\/([a-z0-9-]*)$/.exec(text);
  return match?.[1] ?? null;
}

export function commandOptionId(listId: string, index: number): string {
  return `${listId}-command-${String(index)}`;
}

export interface SlashCommands {
  open: boolean;
  options: readonly PluginCommand[];
  active: number;
  pick: (command: PluginCommand) => void;
  /** Handles the list keys; true when the key was used. */
  onKey: (event: KeyboardEvent<HTMLTextAreaElement>) => boolean;
  /**
   * Sends a typed plugin command to its plugin and empties the box; the text never goes into the
   * chat. False when `text` is no plugin command (send it as usual).
   */
  run: (text: string) => boolean;
  /** "The command could not be run", or `null`. */
  error: string | null;
}

export function useSlashCommands(
  text: string,
  setText: (text: string) => void,
  disabled: boolean,
): SlashCommands {
  const { t } = useTranslation("pluginChat");
  const { commands } = usePluginChat();
  // The highlighted row belongs to the query it was chosen for; a new query starts at the top.
  const [selected, setSelected] = useState<{ query: string | null; index: number }>({
    query: null,
    index: 0,
  });
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const query = disabled ? null : commandQuery(text);
  const options = query === null ? [] : matchCommands(commands, query);
  const open = options.length > 0 && dismissed !== text;
  const active = selected.query === query ? selected.index : 0;
  const index = Math.min(active, Math.max(options.length - 1, 0));
  const setActive = (next: number) => {
    setSelected({ query, index: next });
  };

  const pick = (command: PluginCommand) => {
    setText(`/${command.name} `);
    setDismissed(null);
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
        pick(chosen);
        break;
      case "Enter":
        // A fully typed command runs; a partial one is completed first.
        if (chosen === undefined || event.shiftKey || chosen.name === query) return false;
        pick(chosen);
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
    if (disabled || commandOf(commands, line) === undefined) return false;
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

  return { open, options, active: index, pick, onKey, run, error };
}
