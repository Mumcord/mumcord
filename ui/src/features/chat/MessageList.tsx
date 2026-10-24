/**
 * Scrollable message list: sender/time groups, sanitized bodies, scroll-back paging and a single
 * Tab stop with arrow-key navigation between messages. Links open only on click.
 */
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import type { ChatMessage } from "../../api";
import type { ConversationLog } from "./chatStore";
import { groupStarts } from "./grouping";
import { openLink } from "./openLink";
import { sanitizeHtml } from "./sanitize";

/** Within this distance of the top, older history is loaded; of the bottom, new messages scroll. */
const EDGE_PX = 48;

export interface MessageListProps {
  log: ConversationLog;
  /** Accessible name of the list, e.g. "Messages in Lobby". */
  label: string;
  onLoadOlder: () => void;
  /** Opens the sender's profile card; without it, sender names are plain text. */
  onOpenProfile?: ((session: number, trigger: HTMLElement) => void) | undefined;
}

/** Sanitized message HTML. Its links are Tab stops only while the message is the active one. */
function Body({ html, tabIndex }: { html: string; tabIndex: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const sanitized = useMemo(() => sanitizeHtml(html), [html]);
  useEffect(() => {
    for (const link of ref.current?.querySelectorAll("a") ?? []) {
      link.tabIndex = tabIndex;
    }
  }, [sanitized, tabIndex]);
  return (
    <div ref={ref} className="chat-message__body" dangerouslySetInnerHTML={{ __html: sanitized }} />
  );
}

export function MessageList({ log, label, onLoadOlder, onOpenProfile }: MessageListProps) {
  const { t, i18n } = useTranslation("chat");
  const { messages } = log;
  const scrollRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<(HTMLLIElement | null)[]>([]);
  const atBottom = useRef(true);
  const previous = useRef<{ firstId: string | undefined; height: number }>({
    firstId: undefined,
    height: 0,
  });
  const [focusIndex, setFocusIndex] = useState<number | null>(null);
  const starts = useMemo(() => groupStarts(messages), [messages]);
  const time = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { hour: "2-digit", minute: "2-digit" }),
    [i18n.language],
  );
  const fullTime = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { dateStyle: "full", timeStyle: "short" }),
    [i18n.language],
  );
  const active = Math.min(focusIndex ?? messages.length - 1, messages.length - 1);

  // Keep the reading position when older messages are prepended; follow new ones at the bottom.
  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    const firstId = messages[0]?.id;
    const before = previous.current;
    if (before.firstId !== undefined && firstId !== before.firstId && !atBottom.current) {
      element.scrollTop += element.scrollHeight - before.height;
    } else if (atBottom.current) {
      element.scrollTop = element.scrollHeight;
    }
    previous.current = { firstId, height: element.scrollHeight };
  }, [messages]);

  // One delegated listener: links inside sanitized HTML open in the system browser on click.
  useEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    function onClick(event: MouseEvent) {
      const target = event.target instanceof Element ? event.target.closest("a") : null;
      if (!target || !element?.contains(target)) return;
      event.preventDefault();
      const href = target.getAttribute("href");
      if (href) {
        openLink(href).catch((error: unknown) => {
          console.error("Opening a link failed", error);
        });
      }
    }
    element.addEventListener("click", onClick);
    return () => {
      element.removeEventListener("click", onClick);
    };
  }, []);

  function onScroll() {
    const element = scrollRef.current;
    if (!element) return;
    atBottom.current = element.scrollHeight - element.scrollTop - element.clientHeight < EDGE_PX;
    if (element.scrollTop < EDGE_PX && log.hasMore && !log.loading) onLoadOlder();
  }

  function moveFocus(index: number) {
    const next = Math.max(0, Math.min(index, messages.length - 1));
    setFocusIndex(next);
    itemRefs.current[next]?.focus();
  }

  function onKeyDown(event: KeyboardEvent<HTMLOListElement>) {
    if (event.target !== itemRefs.current[active]) return;
    switch (event.key) {
      case "ArrowUp":
        if (active === 0 && log.hasMore && !log.loading) onLoadOlder();
        moveFocus(active - 1);
        break;
      case "ArrowDown":
        moveFocus(active + 1);
        break;
      case "Home":
        moveFocus(0);
        break;
      case "End":
        moveFocus(messages.length - 1);
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  return (
    <div className="chat-list" ref={scrollRef} onScroll={onScroll}>
      {log.hasMore ? (
        <button
          type="button"
          className="chat-list__older"
          disabled={log.loading}
          onClick={onLoadOlder}
        >
          {log.loading ? t("list.loading") : t("list.loadOlder")}
        </button>
      ) : null}
      {log.loadFailed ? (
        <p className="chat-list__notice" role="status">
          {t("list.loadFailed")}
        </p>
      ) : null}
      {messages.length === 0 ? (
        log.loaded ? (
          <p className="chat-list__empty">{t("empty")}</p>
        ) : (
          <p className="chat-list__notice">{t("list.loading")}</p>
        )
      ) : (
        // Arrow-key navigation lives on the list; each item is a focusable roving stop.
        // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions
        <ol
          className="chat-list__items"
          aria-label={label}
          onKeyDown={onKeyDown}
          onFocus={(event) => {
            const focused: EventTarget = event.target;
            const index = itemRefs.current.findIndex((item) => item === focused);
            if (index >= 0) setFocusIndex(index);
          }}
        >
          {messages.map((message, index) => (
            <MessageItem
              key={message.id}
              message={message}
              start={starts[index] ?? true}
              time={time}
              fullTime={fullTime}
              tabIndex={index === active ? 0 : -1}
              itemRef={(element) => {
                itemRefs.current[index] = element;
              }}
              onOpenProfile={onOpenProfile}
            />
          ))}
        </ol>
      )}
    </div>
  );
}

interface MessageItemProps {
  message: ChatMessage;
  start: boolean;
  time: Intl.DateTimeFormat;
  fullTime: Intl.DateTimeFormat;
  tabIndex: number;
  itemRef: (element: HTMLLIElement | null) => void;
  onOpenProfile: MessageListProps["onOpenProfile"];
}

function MessageItem({
  message,
  start,
  time,
  fullTime,
  tabIndex,
  itemRef,
  onOpenProfile,
}: MessageItemProps) {
  const sentAt = new Date(message.sentAt);
  const session = message.senderSession;
  const sender =
    onOpenProfile && session !== null && !message.own ? (
      <button
        type="button"
        className="chat-message__sender chat-message__sender--button"
        tabIndex={tabIndex}
        onClick={(event) => {
          onOpenProfile(session, event.currentTarget);
        }}
      >
        {message.senderName}
      </button>
    ) : (
      <span className="chat-message__sender">{message.senderName}</span>
    );
  const stamp = (
    <time dateTime={message.sentAt} title={fullTime.format(sentAt)}>
      {time.format(sentAt)}
    </time>
  );
  return (
    // Roving tab stop so screen-reader and keyboard users can step through messages.
    <li
      ref={itemRef}
      className={start ? "chat-message chat-message--start" : "chat-message"}
      tabIndex={tabIndex}
    >
      {start ? (
        <div className="chat-message__header">
          {sender}
          <span className="chat-message__time">{stamp}</span>
        </div>
      ) : (
        <span className="visually-hidden">
          {message.senderName} {stamp}
        </span>
      )}
      <Body html={message.body} tabIndex={tabIndex} />
    </li>
  );
}
