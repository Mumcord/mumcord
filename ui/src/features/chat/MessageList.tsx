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
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import type { ChatDisplay, ChatMessage } from "../../api";
import { useServerSelector } from "../channels";
import { MumcordBadge } from "../channels/MumcordBadge";
import {
  Avatar,
  clickPerson,
  doubleClickPerson,
  openPersonMenu,
  pinPerson,
  pointerDownOnPerson,
  previewOnFocus,
  ProfilePreviewTrigger,
  releasePreview,
  requestPreview,
} from "../members";
import type { ConversationLog } from "./chatStore";
import { daysAgo, messageLayout } from "./grouping";
import { highlightCode } from "./highlight";
import { hideShownLinks, useInlineShown } from "./inlineLinks";
import { ImageViewer, type ViewerImage } from "./ImageViewer";
import { LinkPreviews } from "./LinkPreviewCard";
import { linkMentions, MENTION_CLASS, mentionPeople } from "./mentionChips";
import { imageName, type ImageClickHandler } from "./links";
import { openLink } from "./openLink";
import { useIsMention } from "./ownMention";
import { sanitizeHtml } from "./sanitize";

/** Within this distance of the top, older history is loaded; of the bottom, new messages scroll. */
const EDGE_PX = 48;

export interface MessageListProps {
  log: ConversationLog;
  /** Accessible name of the list, e.g. "Messages in Lobby". */
  label: string;
  onLoadOlder: () => void;
  /**
   * Senders and mention chips react to clicks (pinned card, double click opens the private chat,
   * FR-1.62); without it, sender names are plain text.
   */
  clickablePeople?: boolean | undefined;
  /** Cozy (avatars, grouped) or compact (one line per message). */
  display?: ChatDisplay;
  /** Avatar image of a message's sender, `null` for the initials fallback. */
  avatarFor?: (message: ChatMessage) => string | null;
  /** Show preview cards for links (FR-1.49); off means plain links and no requests. */
  linkPreviews?: boolean;
}

const noAvatar = (): null => null;

/**
 * Sanitized message HTML with highlighted code, plus preview cards for its links when enabled.
 * Its links are Tab stops only while the message is the active one.
 */
interface BodyProps {
  message: ChatMessage;
  tabIndex: number;
  previews: boolean;
  onImageClick: ImageClickHandler;
  clickablePeople: boolean;
}

/** The mention chip an event happened on, with the person it stands for. */
function chipOf(target: EventTarget): { chip: HTMLElement; session: number; name: string } | null {
  const chip = target instanceof Element ? target.closest<HTMLElement>(`.${MENTION_CLASS}`) : null;
  const session = Number(chip?.dataset["mentionSession"]);
  if (!chip || Number.isNaN(session)) return null;
  return { chip, session, name: chip.dataset["mentionName"] ?? "" };
}

function Body({ message, tabIndex, previews, onImageClick, clickablePeople }: BodyProps) {
  const { t } = useTranslation("chat");
  const html = message.body;
  const ref = useRef<HTMLDivElement>(null);
  const cleaned = useMemo(() => highlightCode(sanitizeHtml(html)), [html]);
  const inlineShown = useInlineShown(message.id);
  const users = useServerSelector((state) => state.users);
  // A link whose image is shown below gives way to the image (QA.43).
  const withoutLinks = useMemo(
    () => (previews ? hideShownLinks(cleaned, inlineShown) : cleaned),
    [cleaned, inlineShown, previews],
  );
  // Mentions of people on the server become chips; added after sanitizing, from text nodes only.
  const sanitized = useMemo(
    () =>
      linkMentions(withoutLinks, mentionPeople(users.values()), (name) =>
        t("mention.chipLabel", { name }),
      ),
    [withoutLinks, users, t],
  );
  useEffect(() => {
    for (const chip of ref.current?.querySelectorAll(`.${MENTION_CLASS}`) ?? []) {
      chip.setAttribute("tabindex", String(tabIndex));
    }
    for (const link of ref.current?.querySelectorAll("a") ?? []) {
      link.tabIndex = tabIndex;
    }
    // Images sent inside the message open the viewer; the click is handled on the list.
    for (const image of ref.current?.querySelectorAll("img") ?? []) {
      image.tabIndex = tabIndex;
      // A native drag would serialise the whole data: URL into the drag payload and freeze WebKitGTK.
      image.draggable = false;
      image.setAttribute("role", "button");
      image.setAttribute("aria-label", t("image.viewMessage", { name: message.senderName }));
    }
  }, [sanitized, tabIndex, t, message.senderName]);
  const body = (
    // Delegated handlers for the mention chips inside the sanitized HTML; keyboard use is below.
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <div
      ref={ref}
      className="chat-message__body"
      dangerouslySetInnerHTML={{ __html: sanitized }}
      onPointerOver={(event) => {
        const hit = chipOf(event.target);
        if (hit && event.pointerType !== "touch") {
          requestPreview(hit.session, hit.chip, undefined, hit.name);
        }
      }}
      onPointerOut={(event) => {
        const hit = chipOf(event.target);
        const into = event.relatedTarget instanceof Node ? event.relatedTarget : null;
        if (hit && !hit.chip.contains(into) && event.pointerType !== "touch") releasePreview();
      }}
      onPointerDown={(event) => {
        const hit = chipOf(event.target);
        if (hit) pointerDownOnPerson(hit.session, hit.name);
      }}
      onFocus={(event) => {
        const hit = chipOf(event.target);
        if (hit) previewOnFocus(hit.session, hit.chip, event.target, hit.name);
      }}
      onBlur={(event) => {
        if (chipOf(event.target)) releasePreview();
      }}
      onClick={(event) => {
        const hit = chipOf(event.target);
        if (hit && clickablePeople) clickPerson(event.detail, hit.session, hit.chip, hit.name);
      }}
      onDoubleClick={(event) => {
        const hit = chipOf(event.target);
        if (hit && clickablePeople) doubleClickPerson(hit.session, hit.name);
      }}
      onContextMenu={(event) => {
        const hit = chipOf(event.target);
        if (!hit) return;
        event.preventDefault();
        const rect = hit.chip.getBoundingClientRect();
        const keyboard = event.clientX === 0 && event.clientY === 0;
        openPersonMenu({
          session: hit.session,
          name: hit.name,
          x: keyboard ? rect.left : event.clientX,
          y: keyboard ? rect.bottom : event.clientY,
          opener: hit.chip,
        });
      }}
      onKeyDown={(event) => {
        const hit = chipOf(event.target);
        if (!hit || event.target !== hit.chip) return;
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          event.stopPropagation();
          if (clickablePeople) pinPerson(hit.session, hit.chip, hit.name);
        }
      }}
    />
  );
  if (!previews) return body;
  return (
    <>
      {body}
      <LinkPreviews
        messageId={message.id}
        dismissed={message.dismissedPreviews}
        html={cleaned}
        body={html}
        tabIndex={tabIndex}
        onImageClick={onImageClick}
      />
    </>
  );
}

export function MessageList({
  log,
  label,
  onLoadOlder,
  clickablePeople = false,
  display = "cozy",
  avatarFor = noAvatar,
  linkPreviews = false,
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
  const [viewer, setViewer] = useState<{ images: ViewerImage[]; index: number } | null>(null);
  // Where focus returns when the viewer closes: the clicked image (or its button).
  const viewerOpener = useRef<HTMLElement | null>(null);
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

  // Every image of the conversation in reading order: message images and external inline images.
  // Read from the DOM, so it is exactly what the user sees (dismissed cards are not included).
  const openViewer: ImageClickHandler = (_image, opener) => {
    openViewerAt(opener);
  };

  function openViewerAt(clicked: HTMLElement) {
    const root = scrollRef.current;
    if (!root) return;
    const clickedImage =
      clicked instanceof HTMLImageElement ? clicked : clicked.querySelector("img");
    const images: ViewerImage[] = [];
    let index = 0;
    for (const img of root.querySelectorAll<HTMLImageElement>(
      ".chat-message__body img, .inline-image__img",
    )) {
      if (img === clickedImage) index = images.length;
      images.push({
        src: img.currentSrc || img.src,
        url: img.dataset["imageUrl"],
        name: img.dataset["imageUrl"] ? imageName(img.dataset["imageUrl"]) : t("viewer.unnamed"),
      });
    }
    viewerOpener.current = clicked;
    setViewer({ images, index });
  }

  function closeViewer() {
    setViewer(null);
    const opener = viewerOpener.current;
    // After the dialog is gone; the list may have re-rendered, so fall back to the message.
    setTimeout(() => {
      if (opener?.isConnected) opener.focus();
    }, 0);
  }

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

  function messageImage(target: EventTarget): HTMLImageElement | null {
    const image = target instanceof Element ? target.closest("img") : null;
    return image?.closest(".chat-message__body") ? image : null;
  }

  function onListClick(event: ReactMouseEvent<HTMLDivElement>) {
    const image = messageImage(event.target);
    if (!image) return;
    event.preventDefault();
    openViewerAt(image);
  }

  function onListKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "Enter" && event.key !== " ") return;
    const image = messageImage(event.target);
    if (!image) return;
    event.preventDefault();
    event.stopPropagation();
    openViewerAt(image);
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
    <>
      {/* Clicks on images sent inside messages open the viewer; keyboard users use Enter / Space. */}
      {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
      <div
        className="chat-list"
        ref={scrollRef}
        onScroll={onScroll}
        onLoadCapture={onMediaLoad}
        onClick={onListClick}
        onKeyDown={onListKeyDown}
      >
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
                clickablePeople={clickablePeople}
                linkPreviews={linkPreviews}
                onImageClick={openViewer}
              />
            ))}
          </ol>
        )}
      </div>
      {viewer ? (
        <ImageViewer
          images={viewer.images}
          index={viewer.index}
          onIndexChange={(index) => {
            setViewer({ ...viewer, index });
          }}
          onClose={closeViewer}
        />
      ) : null}
    </>
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
  clickablePeople: boolean;
  linkPreviews: boolean;
  onImageClick: ImageClickHandler;
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
  clickablePeople,
  linkPreviews,
  onImageClick,
}: MessageItemProps) {
  const { t } = useTranslation("chat");
  const mentioned = useIsMention(message);
  const sentAt = new Date(message.sentAt);
  const session = message.senderSession;
  // Avatar and name of every sender open the preview and the context menu. Someone who left (or
  // history without a session) still gets them, limited to the stored name.
  const preview = (content: ReactNode) => (
    <ProfilePreviewTrigger session={session} name={message.senderName}>
      {content}
    </ProfilePreviewTrigger>
  );
  const senderUser = useServerSelector((state) =>
    session === null ? undefined : state.users.get(session),
  );
  const sender =
    clickablePeople && session !== null
      ? preview(
          <button
            type="button"
            className="chat-message__sender chat-message__sender--button"
            tabIndex={tabIndex}
            onClick={(event) => {
              clickPerson(event.detail, session, event.currentTarget, message.senderName);
            }}
            onDoubleClick={() => {
              doubleClickPerson(session, message.senderName);
            }}
          >
            {message.senderName}
            <MumcordBadge user={senderUser} />
          </button>,
        )
      : preview(
          <span className="chat-message__sender">
            {message.senderName}
            <MumcordBadge user={senderUser} />
          </span>,
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
        {stamp("chat-message__time")} {sender}{" "}
        <Body
          message={message}
          tabIndex={tabIndex}
          previews={linkPreviews}
          onImageClick={onImageClick}
          clickablePeople={clickablePeople}
        />
      </div>
    );
  } else if (start) {
    row = (
      <div className="chat-message__row">
        <div className="chat-message__gutter">
          {preview(
            clickablePeople && session !== null ? (
              // Mouse shortcut only; the sender name next to it is the keyboard route.
              // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions
              <span
                onClick={(event) => {
                  clickPerson(event.detail, session, event.currentTarget, message.senderName);
                }}
                onDoubleClick={() => {
                  doubleClickPerson(session, message.senderName);
                }}
              >
                <Avatar name={message.senderName} src={avatar} />
              </span>
            ) : (
              <Avatar name={message.senderName} src={avatar} />
            ),
          )}
        </div>
        <div className="chat-message__content">
          <div className="chat-message__header">
            {sender}
            {stamp("chat-message__time")}
          </div>
          <Body
            message={message}
            tabIndex={tabIndex}
            previews={linkPreviews}
            onImageClick={onImageClick}
            clickablePeople={clickablePeople}
          />
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
          <Body
            message={message}
            tabIndex={tabIndex}
            previews={linkPreviews}
            onImageClick={onImageClick}
            clickablePeople={clickablePeople}
          />
        </div>
      </div>
    );
  }

  return (
    // Roving tab stop so screen-reader and keyboard users can step through messages.
    <li
      ref={itemRef}
      className={
        (start ? "chat-message chat-message--start" : "chat-message") +
        (mentioned ? " chat-message--mention" : "")
      }
      tabIndex={tabIndex}
    >
      {mentioned ? <span className="visually-hidden">{t("mention.mentionsYou")}. </span> : null}
      {day !== null ? <div className="chat-day">{day}</div> : null}
      {row}
    </li>
  );
}
