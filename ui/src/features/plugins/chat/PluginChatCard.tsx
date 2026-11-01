/**
 * A plugin chat card (FR-6.40 `chat-card`, 440×160): a local item in the chat, drawn with the
 * plugin's tree. Shows nothing once the plugin clears it or stops.
 */
import { useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ChatMessage, PluginSlot } from "../../../api";
import { PluginTree } from "../render/PluginTree";
import { usePluginTree } from "../render/pluginUiStore";
import {
  cardsAfter,
  channelOfListKey,
  leadingCards,
  usePluginChat,
  viaPlugin,
  type PlacedCard,
} from "./pluginChatStore";
import "./pluginChat.css";

export function PluginChatCard({ plugin, card }: { plugin: string; card: string }) {
  const { t } = useTranslation("pluginChat");
  const { names } = usePluginChat();
  const slot = useMemo<PluginSlot>(() => ({ kind: "chatCard", id: card }), [card]);
  const nodes = usePluginTree(plugin, slot);
  if (nodes === undefined) return null;
  return (
    <section
      className="plugin-chat-card"
      aria-label={t("card.label", { plugin: names.get(plugin) ?? plugin })}
    >
      <PluginTree plugin={plugin} slot={slot} nodes={nodes} />
    </section>
  );
}

function Cards({ cards }: { cards: readonly PlacedCard[] }) {
  return cards.map((c) => (
    <PluginChatCard key={`${c.plugin}:${c.card}`} plugin={c.plugin} card={c.card} />
  ));
}

/**
 * Wraps a chat message's body: a line replaced by a plugin card shows the card with "Show text"
 * (FR-6.45), a line a plugin posted gets a "via <plugin>" label (FR-6.44), and cards that
 * appeared after this message follow it.
 */
export function PluginLine({ message, children }: { message: ChatMessage; children: ReactNode }) {
  const { t } = useTranslation("pluginChat");
  const chat = usePluginChat();
  const replaced = chat.superseded.get(message.id);
  const after = cardsAfter(chat, message);
  const via = viaPlugin(chat.names, message.body);
  const content =
    replaced === undefined ? (
      children
    ) : (
      <SupersededLine plugin={replaced.plugin} card={replaced.card}>
        {children}
      </SupersededLine>
    );
  return (
    <>
      {via !== null && replaced === undefined ? (
        <span className="plugin-chat-via">{t("via", { plugin: via })}</span>
      ) : null}
      {content}
      <Cards cards={after} />
    </>
  );
}

/** The card in place of its fallback line; the line stays one click away. */
function SupersededLine({
  plugin,
  card,
  children,
}: {
  plugin: string;
  card: string;
  children: ReactNode;
}) {
  const { t } = useTranslation("pluginChat");
  const slot = useMemo<PluginSlot>(() => ({ kind: "chatCard", id: card }), [card]);
  const nodes = usePluginTree(plugin, slot);
  const [shown, setShown] = useState(false);
  // Without the card (cleared, plugin stopped) the line shows normally.
  if (nodes === undefined) return children;
  return (
    <>
      <PluginChatCard plugin={plugin} card={card} />
      <button
        type="button"
        className="plugin-chat-show-text"
        aria-expanded={shown}
        onClick={() => {
          setShown(!shown);
        }}
      >
        {shown ? t("supersede.hideText") : t("supersede.showText")}
      </button>
      {shown ? children : null}
    </>
  );
}

/** Cards in a channel that had no message yet when they appeared; shown above the list. */
export function LeadingPluginCards({ listKey }: { listKey: string }) {
  const chat = usePluginChat();
  const channel = channelOfListKey(listKey);
  if (channel === null) return null;
  const cards = leadingCards(chat, channel.serverId, channel.channelId);
  if (cards.length === 0) return null;
  return (
    <div className="plugin-chat-leading">
      <Cards cards={cards} />
    </div>
  );
}
