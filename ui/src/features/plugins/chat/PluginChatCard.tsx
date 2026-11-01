/**
 * A plugin chat card (FR-6.40 `chat-card`, 440×160): a local item in the chat, drawn with the
 * plugin's tree. Shows nothing once the plugin clears it or stops.
 */
import {
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import type { ChatMessage, PluginSlot } from "../../../api";
import { PluginTree } from "../render/PluginTree";
import { usePluginTree } from "../render/pluginUiStore";
import { useShownServerId } from "../../channels/serverStore";
import { usePluginIcon } from "../render/usePluginIcon";
import {
  cardsAfter,
  channelOfListKey,
  leadingCards,
  reanchorChannelCards,
  replayChannel,
  usePluginChat,
  viaPlugin,
  type PlacedCard,
} from "./pluginChatStore";
import "./pluginChat.css";

export function PluginChatCard({
  plugin,
  card,
  marked = false,
}: {
  plugin: string;
  card: string;
  /** Replaces a message: the header names the plugin with its icon, drawn by the host. */
  marked?: boolean;
}) {
  const { t } = useTranslation("pluginChat");
  const { names } = usePluginChat();
  const slot = useMemo<PluginSlot>(() => ({ kind: "chatCard", id: card }), [card]);
  const nodes = usePluginTree(plugin, slot);
  // Cards show only in the chat on screen: replies to their buttons go to that server.
  const serverId = useShownServerId();
  if (nodes === undefined) return null;
  const name = names.get(plugin) ?? plugin;
  return (
    <section className="plugin-chat-card" aria-label={t("card.label", { plugin: name })}>
      {marked ? (
        <header className="plugin-chat-card__header">
          <PluginMark plugin={plugin} name={name} />
          <span>{name}</span>
        </header>
      ) : null}
      <CardBody>
        <PluginTree plugin={plugin} slot={slot} nodes={nodes} serverId={serverId} />
      </CardBody>
    </section>
  );
}

/**
 * Keeps the card at its 160 px slot height. A scrollbar inside a chat line is easy to miss and
 * fights the chat's own scrolling, so taller content is clipped with a fade and "Show more"
 * lets the card grow to its full height instead.
 */
function CardBody({ children }: { children: ReactNode }) {
  const { t } = useTranslation("pluginChat");
  const clip = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const [overflows, setOverflows] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const id = useId();
  useLayoutEffect(() => {
    const outer = clip.current;
    const inner = content.current;
    if (outer === null || inner === null || expanded) return;
    const measure = () => {
      setOverflows(inner.offsetHeight > outer.clientHeight);
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(inner);
    return () => {
      observer.disconnect();
    };
  }, [expanded]);
  const clipped = overflows && !expanded;
  return (
    <>
      <div
        id={id}
        ref={clip}
        className={`plugin-chat-card__body${expanded ? " plugin-chat-card__body--expanded" : ""}${clipped ? " plugin-chat-card__body--clipped" : ""}`}
      >
        <div ref={content}>{children}</div>
      </div>
      {overflows ? (
        <button
          type="button"
          className="plugin-chat-show-text plugin-chat-card__more"
          aria-expanded={expanded}
          aria-controls={id}
          onClick={() => {
            setExpanded(!expanded);
          }}
        >
          {expanded ? t("card.showLess") : t("card.showMore")}
        </button>
      ) : null}
    </>
  );
}

/** The plugin's package icon, or its initial; decorative, the name is always next to it. */
export function PluginMark({ plugin, name }: { plugin: string; name: string }) {
  const icon = usePluginIcon(plugin);
  return icon === null ? (
    <span className="plugin-chat-mark plugin-chat-mark--initial" aria-hidden="true">
      {(name.trim()[0] ?? "?").toUpperCase()}
    </span>
  ) : (
    <img className="plugin-chat-mark" src={icon} alt="" />
  );
}

function Cards({ cards }: { cards: readonly PlacedCard[] }) {
  return cards.map((c) => (
    <PluginChatCard key={`${c.plugin}:${c.card}`} plugin={c.plugin} card={c.card} />
  ));
}

/**
 * Wraps a chat message's body: a line replaced by a plugin card shows the card with "Show
 * original text", a line a plugin hid shows "Hidden by <plugin>" (FR-6.45), a line a plugin
 * posted gets a "via <plugin>" label (FR-6.44), and cards that appeared after this message
 * follow it.
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
    ) : replaced.card === null ? (
      <HiddenLine plugin={replaced.plugin}>{children}</HiddenLine>
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

/** The card in place of the message; the original text stays one click away. */
function SupersededLine({
  plugin,
  card,
  children,
}: {
  plugin: string;
  card: string;
  children: ReactNode;
}) {
  const slot = useMemo<PluginSlot>(() => ({ kind: "chatCard", id: card }), [card]);
  const nodes = usePluginTree(plugin, slot);
  // Without the card (cleared, plugin stopped) the line shows normally.
  if (nodes === undefined) return children;
  return (
    <>
      <PluginChatCard plugin={plugin} card={card} marked />
      <Original>{children}</Original>
    </>
  );
}

/** A message a plugin hid (e.g. a counted vote): who hid it, the text one click away. */
function HiddenLine({ plugin, children }: { plugin: string; children: ReactNode }) {
  const { t } = useTranslation("pluginChat");
  const { names } = usePluginChat();
  const name = names.get(plugin) ?? plugin;
  return (
    <Original
      label={
        <span className="plugin-chat-hidden">
          <PluginMark plugin={plugin} name={name} />
          {t("supersede.hiddenBy", { plugin: name })}
        </span>
      }
    >
      {children}
    </Original>
  );
}

/** "Show original text" / "Hide original text" and the original message below it. */
export function Original({ label, children }: { label?: ReactNode; children: ReactNode }) {
  const { t } = useTranslation("pluginChat");
  const [shown, setShown] = useState(false);
  const id = useId();
  return (
    <>
      <div className="plugin-chat-original-bar">
        {label}
        <button
          type="button"
          className="plugin-chat-show-text"
          aria-expanded={shown}
          aria-controls={id}
          onClick={() => {
            setShown(!shown);
          }}
        >
          {shown ? t("supersede.hideText") : t("supersede.showText")}
        </button>
      </div>
      <div id={id} hidden={!shown}>
        {shown ? children : null}
      </div>
    </>
  );
}

/**
 * Cards in a channel that had no message yet when they appeared; shown above the list. Once the
 * channel's history is loaded, and again whenever the plugins change, the plugins see its stored
 * messages again so old messages get their cards too.
 */
export function LeadingPluginCards({
  listKey,
  loaded,
  messages = [],
}: {
  listKey: string;
  loaded: boolean;
  messages?: readonly ChatMessage[];
}) {
  const chat = usePluginChat();
  const channel = channelOfListKey(listKey);
  const serverId = channel?.serverId;
  const channelId = channel?.channelId;
  useEffect(() => {
    if (loaded && serverId !== undefined && channelId !== undefined) {
      replayChannel(serverId, channelId);
    }
  }, [loaded, serverId, channelId, chat.names]);
  useEffect(() => {
    if (serverId !== undefined && channelId !== undefined && messages.length > 0) {
      reanchorChannelCards(serverId, channelId, messages);
    }
  }, [serverId, channelId, messages]);
  if (channel === null) return null;
  const cards = leadingCards(chat, channel.serverId, channel.channelId);
  if (cards.length === 0) return null;
  return (
    <div className="plugin-chat-leading">
      <Cards cards={cards} />
    </div>
  );
}
