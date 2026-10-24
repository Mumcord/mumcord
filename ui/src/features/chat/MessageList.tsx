/**
 * Scrollable message list (FR-1.48a): Discord-style groups (avatar, name and time once per group,
 * text indented after the avatar) or a compact one-line layout, day separators, sanitized bodies,
 * scroll-back paging and a single Tab stop with arrow-key navigation between messages. Links open
 * only on click.
 */
import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import type { ChatDisplay, ChatMessage } from "../../api";
import { Avatar, ProfilePreviewTrigger } from "../members";
import type { ConversationLog } from "./chatStore";
import { daysAgo, messageLayout } from "./grouping";
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
  /** Cozy (avatars, grouped) or compact (one line per message). */
  display?: ChatDisplay;
  /** Avatar image of a message's sender, `null` for the initials fallback. */
  avatarFor?: (message: ChatMessage) => string | null;
}

const noAvatar = (): null => null;

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

export function MessageList({
  log,
  label,
  onLoadOlder,
  onOpenProfile,
  display = "cozy",
  avatarFor = noAvatar,
}: MessageListProps) {
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
  const layout = useMemo(() => messageLayout(messages), [messages]);
  const time = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { hour: "2-digit", minute: "2-digit" }),
    [i18n.language],
  );
  const fullTime = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { dateStyle: "full", timeStyle: "short" }),
    [i18n.language],
  );
  const dayFormat = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { dateStyle: "long" }),
    [i18n.language],
  );
  const now = new Date();
  const dayLabel = (date: Date): string => {
    const ago = daysAgo(date, now);
    if (ago === 0) return t("list.today");
    if (ago === 1) return t("list.yesterday");
    return dayFormat.format(date);
  };
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

  // Images load after render and grow the list; keep following the bottom by scrolling only the list.
  function onMediaLoad() {
    const element = scrollRef.current;
    if (element && atBottom.current) element.scrollTop = element.scrollHeight;
  }

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
    <div className="chat-list" ref={scrollRef} onScroll={onScroll} onLoadCapture={onMediaLoad}>
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
          data-display={display}
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
              start={layout[index]?.start ?? true}
              day={layout[index]?.newDay ? dayLabel(new Date(message.sentAt)) : null}
              display={display}
              avatar={display === "cozy" && layout[index]?.start ? avatarFor(message) : null}
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
  /** Day separator label above this message, if it is the first of its day. */
  day: string | null;
  display: ChatDisplay;
  avatar: string | null;
  time: Intl.DateTimeFormat;
  fullTime: Intl.DateTimeFormat;
  tabIndex: number;
  itemRef: (element: HTMLLIElement | null) => void;
  onOpenProfile: MessageListProps["onOpenProfile"];
}

function MessageItem({
  message,
  start,
  day,
  display,
  avatar,
  time,
  fullTime,
  tabIndex,
  itemRef,
  onOpenProfile,
}: MessageItemProps) {
  const sentAt = new Date(message.sentAt);
  const session = message.senderSession;
  // Live senders other than oneself get the profile preview on hover/focus of avatar and name.
  const preview = (content: ReactNode) =>
    session !== null && !message.own ? (
      <ProfilePreviewTrigger session={session}>{content}</ProfilePreviewTrigger>
    ) : (
      content
    );
  const sender =
    onOpenProfile && session !== null && !message.own ? (
      preview(
        <button
          type="button"
          className="chat-message__sender chat-message__sender--button"
          tabIndex={tabIndex}
          onClick={(event) => {
            onOpenProfile(session, event.currentTarget);
          }}
        >
          {message.senderName}
        </button>,
      )
    ) : (
      <span className="chat-message__sender">{message.senderName}</span>
    );
  const stamp = (className: string) => (
    <time className={className} dateTime={message.sentAt} title={fullTime.format(sentAt)}>
      {time.format(sentAt)}
    </time>
  );

  let row;
  if (display === "compact") {
    row = (
      <div className="chat-message__row">
        {stamp("chat-message__time")} {sender} <Body html={message.body} tabIndex={tabIndex} />
      </div>
    );
  } else if (start) {
    row = (
      <div className="chat-message__row">
        <div className="chat-message__gutter">
          {preview(<Avatar name={message.senderName} src={avatar} />)}
        </div>
        <div className="chat-message__content">
          <div className="chat-message__header">
            {sender}
            {stamp("chat-message__time")}
          </div>
          <Body html={message.body} tabIndex={tabIndex} />
        </div>
      </div>
    );
  } else {
    // Grouped: the sender stays readable for screen readers; the time shows on hover/focus.
    row = (
      <div className="chat-message__row">
        <span className="chat-message__sender visually-hidden">{message.senderName} </span>
        <div className="chat-message__gutter">{stamp("chat-message__hover-time")}</div>
        <div className="chat-message__content">
          <Body html={message.body} tabIndex={tabIndex} />
        </div>
      </div>
    );
  }

  return (
    // Roving tab stop so screen-reader and keyboard users can step through messages.
    <li
      ref={itemRef}
      className={start ? "chat-message chat-message--start" : "chat-message"}
      tabIndex={tabIndex}
    >
      {day !== null ? <div className="chat-day">{day}</div> : null}
      {row}
    </li>
  );
}
