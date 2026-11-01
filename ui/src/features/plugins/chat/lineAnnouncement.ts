/**
 * What a screen reader should say for a new chat line once plugins had their say (FR-6.45): a
 * line a plugin replaced with a card is announced as that card, a line a plugin hid is not
 * announced at all, like it is not shown.
 */
import type { TFunction } from "i18next";
import type { PluginNode } from "../../../api";
import { getPluginUi, slotKey, type PluginUiState } from "../render/pluginUiStore";
import { getPluginChat, type PluginChatState } from "./pluginChatStore";

/**
 * How long a new line waits for a plugin to replace or hide it. Plugins see the line at about
 * the same time as the chat, so their answer usually follows within a few milliseconds.
 */
export const SUPERSEDE_GRACE_MS = 500;

const SUMMARY_CHARS = 100;

/** How a line shows: as itself, hidden, or as a plugin card. */
export type LineView =
  { kind: "line" } | { kind: "hidden" } | { kind: "card"; plugin: string; summary: string | null };

/** The first text of a card, e.g. a poll's question. */
function cardSummary(nodes: readonly PluginNode[]): string | null {
  for (const node of nodes) {
    const c = node.component;
    if (c.type === "text" && "text" in c) {
      const text = c.text.trim();
      if (text !== "") return text.slice(0, SUMMARY_CHARS);
    }
  }
  return null;
}

export function lineView(chat: PluginChatState, ui: PluginUiState, messageId: string): LineView {
  const replaced = chat.superseded.get(messageId);
  if (replaced === undefined) return { kind: "line" };
  if (replaced.card === null) return { kind: "hidden" };
  const nodes = ui.trees
    .get(replaced.plugin)
    ?.get(slotKey({ kind: "chatCard", id: replaced.card }));
  // Without its card the line shows normally (see PluginLine), so it is read normally too.
  if (nodes === undefined) return { kind: "line" };
  return {
    kind: "card",
    plugin: chat.names.get(replaced.plugin) ?? replaced.plugin,
    summary: cardSummary(nodes),
  };
}

/** "Card from Poll: Lunch at noon?", what a replaced line is read as. */
export function cardAnnouncement(t: TFunction, view: Extract<LineView, { kind: "card" }>): string {
  const label = t("card.label", { ns: "pluginChat", plugin: view.plugin });
  return view.summary === null ? label : `${label}: ${view.summary}`;
}

export interface LineAnnouncer {
  /** Calls `speak` with how message `messageId` shows, once that is settled; never for hidden lines. */
  resolve(messageId: string, speak: (view: Exclude<LineView, { kind: "hidden" }>) => void): void;
  dispose(): void;
}

export function createLineAnnouncer(
  lookup: (messageId: string) => LineView = (id) => lineView(getPluginChat(), getPluginUi(), id),
  hasPlugins: () => boolean = () => getPluginChat().names.size > 0,
  graceMs = SUPERSEDE_GRACE_MS,
): LineAnnouncer {
  const timers = new Set<ReturnType<typeof setTimeout>>();

  function settle(messageId: string, speak: (view: Exclude<LineView, { kind: "hidden" }>) => void) {
    const view = lookup(messageId);
    if (view.kind !== "hidden") speak(view);
  }

  return {
    resolve(messageId, speak) {
      // Without plugins nothing replaces lines: no reason to delay the announcement.
      if (!hasPlugins()) {
        settle(messageId, speak);
        return;
      }
      const timer = setTimeout(() => {
        timers.delete(timer);
        settle(messageId, speak);
      }, graceMs);
      timers.add(timer);
    },
    dispose() {
      for (const timer of timers) clearTimeout(timer);
      timers.clear();
    },
  };
}
