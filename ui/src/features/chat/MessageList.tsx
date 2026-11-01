/**
 * Scrollable message list (FR-1.48a): grouped messages (avatar, name and time once per group,
 * text indented after the avatar) or a compact one-line layout, day separators, sanitized bodies,
 * scroll-back paging, messages synced from other Mumcord users marked with their origin, a kept
 * reading position when messages are inserted above (FR-1.60b), a "Jump to newest" button and a
 * single Tab stop with arrow-key navigation between messages. Links open only on click.
 * Every message can be replied to (FR-2.60: hover button, context menu or R); a reply shows a
 * compact header with the quote that jumps to the original while it is loaded.
 */
import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ClipboardEvent,
  type KeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { useT } from "../../i18n/useT";
import { getApi, type ChatDisplay, type ChatMessage } from "../../api";
import { useServerSelector } from "../channels";
import { shownServerId } from "../channels/serverStore";
import { useServers } from "../servers/store";
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
import { useMessageWindow } from "./messageWindow";
import { OUTGOING_PREFIX, renderKey, type ConversationLog, type OutgoingSend } from "./chatStore";
import { daysAgo, messageLayout } from "./grouping";
import { pinnedIndex } from "./pinnedAuthor";
import { useCodeBlockActions } from "./useCodeBlockActions";
import { useHighlightedCode } from "./highlight";
import { hideShownLinks, useInlineShown } from "./inlineLinks";
import { fullImageSource } from "./fullImage";
import { FullImageBars } from "./FullImageBars";
import { SendFailure } from "./SendFailure";
import { findThemeCode } from "../../theme/shareCodec";
import { ThemeCard } from "./ThemeCard";
import { extractCards } from "./cards";
import { MessageCards } from "./MessageCard";
import { useSharedTheme, withoutThemeShare } from "./sharedTheme";
import { ImageViewer, type ViewerImage } from "./ImageViewer";
import { LinkPreviews } from "./LinkPreviewCard";
import { linkMentions, MENTION_CLASS, mentionPeopleOf } from "./mentionChips";
import { originalSrc } from "./displaySrc";
import { imageName, type ImageClickHandler } from "./links";
import { openInBrowser, openLink } from "./openLink";
import { useIsMention } from "./ownMention";
import { ContextMenu, type MenuPosition } from "../../components/ContextMenu";
import { findOriginal, parseReply, quoteFor, replyOf, type ReplyQuote } from "./reply";
import { SAFE_LINK, sanitizeHtml } from "./sanitize";
import { ImageMenu, type ImageMenuState } from "./ImageMenu";
import { selectedText } from "./selectionText";
import { revealDone, usePendingReveal } from "./reveal";
import { CollapsedMessage } from "../blocking/CollapsedMessage";
import { LeadingPluginCards, PluginLine } from "../plugins/chat";

/** Within this distance of the top, older history is loaded; of the bottom, new messages scroll. */
const EDGE_PX = 48;
/** How long the original of a reply stays highlighted after jumping to it. */
const HIGHLIGHT_MS = 2000;

export interface MessageListProps {
  log: ConversationLog;
  /** Accessible name of the list, e.g. "Messages in Lobby". */
  label: string;
  onLoadOlder: () => void;
  /**
   * Asks other Mumcord users for earlier messages once the local history is exhausted
   * (FR-1.60); without it, there is no "Load earlier messages" button.
   */
  onSyncEarlier?: (() => void) | undefined;
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
  /** Starts a reply to a message (FR-2.60); without it, messages offer no reply action. */
  onReply?: ((quote: ReplyQuote) => void) | undefined;
  /** Identifies the conversation (and its server); a newly shown one draws its newest first. */
  listKey?: string;
  /**
   * Ids of messages from blocked users drawn collapsed (issue #140) until the user shows one.
   */
  collapsedIds?: ReadonlySet<string> | undefined;
}

/** Messages drawn when a conversation is shown, and drawn more each time the user scrolls up. */
const MESSAGE_WINDOW = 30;

/** Banner height assumed before it has been rendered once. */
const PINNED_BANNER_FALLBACK_PX = 36;

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
  /** Scrolls to the original of a reply; `null` when it is not in the loaded history. */
  onJump: (() => void) | null;
}

/** The mention chip an event happened on, with the person it stands for. */
function chipOf(target: EventTarget): { chip: HTMLElement; session: number; name: string } | null {
  const chip = target instanceof Element ? target.closest<HTMLElement>(`.${MENTION_CLASS}`) : null;
  const session = Number(chip?.dataset["mentionSession"]);
  if (!chip || Number.isNaN(session)) return null;
  return { chip, session, name: chip.dataset["mentionName"] ?? "" };
}

// A message that is only a theme share line has nothing left to show; no empty box then.
function isBlank(html: string): boolean {
  const probe = document.createElement("div");
  probe.innerHTML = html;
  return probe.textContent.trim() === "" && probe.querySelector("img, video, audio, svg") === null;
}

/** The quote of a reply (FR-2.60), shown as plain text; a button when the original is loaded. */
function ReplyHeader({
  quote,
  onJump,
  tabIndex,
}: {
  quote: ReplyQuote;
  onJump: (() => void) | null;
  tabIndex: number;
}) {
  const { t } = useTranslation("chat");
  const content = (
    <>
      <ReplyIcon size={14} />
      <span className="visually-hidden">{t("reply.replyTo")}</span>{" "}
      <span className="chat-reply__name">{quote.name}</span>{" "}
      <span className="chat-reply__text">{quote.snippet}</span>
    </>
  );
  return onJump ? (
    <button
      type="button"
      className="chat-reply chat-reply--jump"
      tabIndex={tabIndex}
      data-tooltip={t("reply.jump")}
      onClick={onJump}
    >
      {content}
      <span className="visually-hidden">. {t("reply.jump")}</span>
    </button>
  ) : (
    <div className="chat-reply">{content}</div>
  );
}

function ReplyIcon({ size }: { size: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} aria-hidden="true" focusable="false">
      <path
        d="M9 14L4 9l5-5M4 9h10a6 6 0 016 6v4"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function Body({ message, tabIndex, previews, onImageClick, clickablePeople, onJump }: BodyProps) {
  const { t } = useTranslation("chat");
  const html = message.body;
  const ref = useRef<HTMLDivElement>(null);
  // Cards (issue #72) render on their own below the text, like the theme card.
  const extracted = useMemo(() => extractCards(sanitizeHtml(html)), [html]);
  const sanitizedHtml = useHighlightedCode(extracted.rest);
  // A reply shows its quote as a header; the rest is the message itself.
  const reply = useMemo(() => parseReply(sanitizedHtml), [sanitizedHtml]);
  const cleaned = reply?.rest ?? sanitizedHtml;
  const inlineShown = useInlineShown(message.id);
  const users = useServerSelector((state) => state.users);
  // A link whose image is shown below gives way to the image (QA.43).
  const sharedTheme = useSharedTheme(cleaned);
  // The theme card replaces the raw share text (FR-2.16); an unusable code stays visible.
  const shown = useMemo(() => {
    const code = sharedTheme === null ? null : findThemeCode(cleaned);
    return code === null ? cleaned : withoutThemeShare(cleaned, code);
  }, [cleaned, sharedTheme]);
  const withoutLinks = useMemo(
    () => (previews ? hideShownLinks(shown, inlineShown) : shown),
    [shown, inlineShown, previews],
  );
  // Mentions of people on the server become chips; added after sanitizing, from text nodes only.
  const sanitized = useMemo(
    () =>
      linkMentions(withoutLinks, mentionPeopleOf(users), (name) =>
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
      // The viewer fetches a full image from the message's sender (FR-1.60c).
      if (message.senderSession !== null)
        image.dataset["senderSession"] = String(message.senderSession);
      if (message.senderCertHash !== null) image.dataset["senderCert"] = message.senderCertHash;
    }
  }, [sanitized, tabIndex, t, message.senderName, message.senderSession, message.senderCertHash]);
  const codeActions = useCodeBlockActions(ref, sanitized, tabIndex);
  const body = isBlank(sanitized) ? null : (
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
  return (
    <PluginLine message={message}>
      {reply ? <ReplyHeader quote={reply.quote} onJump={onJump} tabIndex={tabIndex} /> : null}
      {body}
      {codeActions}
      <MessageCards
        cards={extracted.cards}
        tabIndex={tabIndex}
        previews={previews}
        onImageClick={onImageClick}
      />
      <FullImageBars html={cleaned} />
      <ThemeCard html={cleaned} tabIndex={tabIndex} />
      <LinkPreviews
        messageId={message.id}
        dismissed={message.dismissedPreviews}
        html={cleaned}
        body={html}
        linkPreviews={previews}
        tabIndex={tabIndex}
        onImageClick={onImageClick}
      />
    </PluginLine>
  );
}

export function MessageList({
  log,
  label,
  onLoadOlder,
  onSyncEarlier,
  clickablePeople = false,
  display = "cozy",
  avatarFor = noAvatar,
  linkPreviews = false,
  onReply,
  listKey = "",
  collapsedIds,
}: MessageListProps) {
  const { t, i18n } = useTranslation("chat");
  // Own messages not confirmed yet show at the end, greyed (FR-2.74).
  const outgoing = log.outgoing;
  const allMessages = useMemo(
    () =>
      outgoing?.length ? [...log.messages, ...outgoing.map((item) => item.message)] : log.messages,
    [log.messages, outgoing],
  );
  const {
    shown: messages,
    hidden,
    showOlder,
  } = useMessageWindow(allMessages, listKey, MESSAGE_WINDOW);
  // Loaded messages not drawn yet come first, then older history.
  const canLoadOlder = hidden > 0 || (log.hasMore && !log.loading);
  const loadOlder = () => {
    if (hidden > 0) showOlder();
    else onLoadOlder();
  };
  const outgoingById = useMemo(
    () => new Map((outgoing ?? []).map((item) => [item.message.id, item])),
    [outgoing],
  );
  const scrollRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<(HTMLLIElement | null)[]>([]);
  const bannerRef = useRef<HTMLDivElement>(null);
  const [pinned, setPinned] = useState<number | null>(null);
  const atBottom = useRef(true);
  // The message at the top of the view and its distance from the list's top edge, kept in place
  // when messages are inserted above it.
  const anchor = useRef<{ id: string; offset: number } | null>(null);
  const newestId = useRef<string | undefined>(undefined);
  const shownKey = useRef(listKey);
  // Scrolled away from the latest message, and how many arrived below since then.
  const [away, setAway] = useState(false);
  const [unseen, setUnseen] = useState(0);
  const [focusIndex, setFocusIndex] = useState<number | null>(null);
  const [viewer, setViewer] = useState<{ images: ViewerImage[]; index: number } | null>(null);
  // Where focus returns when the viewer closes: the clicked image (or its button).
  const viewerOpener = useRef<HTMLElement | null>(null);
  const layout = useMemo(() => messageLayout(messages), [messages]);
  // `selection` is the text selected when the menu opened: clicking an item may clear it.
  const [menu, setMenu] = useState<(MenuPosition & { index: number; selection: string }) | null>(
    null,
  );
  const [linkMenu, setLinkMenu] = useState<
    (MenuPosition & { url: string; selection: string }) | null
  >(null);
  const [imageMenu, setImageMenu] = useState<
    (ImageMenuState & { image: HTMLImageElement | null }) | null
  >(null);
  const [copyNote, setCopyNote] = useState<string | null>(null);
  // The original of a reply that was jumped to, highlighted for a moment.
  const [highlighted, setHighlighted] = useState<string | null>(null);
  // Collapsed messages of blocked users the user chose to show.
  const [shownBlocked, setShownBlocked] = useState<ReadonlySet<string>>(() => new Set());
  const serverLabel = useServers().servers.find((s) => s.id === shownServerId())?.label;
  const serverName = serverLabel ?? t("list.server");
  const imageLabel = t("reply.image");
  // Each message's sender as shown; server messages have no sender name.
  const names = useMemo(
    () =>
      messages.map((m) =>
        m.senderName === "" && m.senderSession === null && !m.own ? serverName : m.senderName,
      ),
    [messages, serverName],
  );
  // For each reply, where its original is in the loaded history.
  const originals = useMemo(() => {
    const nameAt = (index: number) => names[index] ?? "";
    return messages.map((message, index) => {
      const quote = replyOf(message)?.quote;
      return quote ? findOriginal(messages, index, quote, nameAt, imageLabel) : null;
    });
  }, [messages, names, imageLabel]);
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

  // FR-2.62: a purely visual banner repeats the author of a tall message whose header scrolled out.
  function measurePinned() {
    const element = scrollRef.current;
    if (!element) return;
    const top = element.getBoundingClientRect().top;
    const items = itemRefs.current.slice(0, messages.length).map((item) => {
      if (!item) return null;
      const rect = item.getBoundingClientRect();
      const header = item.querySelector(".chat-message__header");
      return {
        top: rect.top,
        bottom: rect.bottom,
        headerBottom: header ? header.getBoundingClientRect().bottom : null,
      };
    });
    const height = bannerRef.current?.offsetHeight ?? PINNED_BANNER_FALLBACK_PX;
    const next = pinnedIndex(items, top, height);
    setPinned((current) => (current === next ? current : next));
  }

  function measureAnchor() {
    const element = scrollRef.current;
    if (!element) return;
    const top = element.getBoundingClientRect().top;
    anchor.current = null;
    for (const [index, item] of itemRefs.current.entries()) {
      const id = messages[index]?.id;
      if (!item || id === undefined) continue;
      const rect = item.getBoundingClientRect();
      if (rect.bottom > top) {
        anchor.current = { id, offset: rect.top - top };
        return;
      }
    }
  }

  // Follow new messages at the bottom; otherwise keep the message at the top of the view where it
  // was, whatever was inserted above it (older pages, synced history), and count new arrivals.
  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    itemRefs.current.length = messages.length;
    // Another conversation (or server) opens at its newest message, not where the last one was.
    if (listKey !== shownKey.current) {
      shownKey.current = listKey;
      atBottom.current = true;
      setAway(false);
      setUnseen(0);
    }
    const newest = messages[messages.length - 1]?.id;
    // The user just sent a message: show it, wherever they had scrolled to.
    const sent = newest !== newestId.current && newest?.startsWith(OUTGOING_PREFIX) === true;
    if (sent) {
      atBottom.current = true;
      setAway(false);
      setUnseen(0);
    }
    if (atBottom.current) {
      element.scrollTop = element.scrollHeight;
    } else {
      const kept = anchor.current;
      const item = kept ? itemRefs.current[messages.findIndex((m) => m.id === kept.id)] : null;
      if (kept && item) {
        const offset = item.getBoundingClientRect().top - element.getBoundingClientRect().top;
        element.scrollTop += offset - kept.offset;
      }
      const previous = messages.findIndex((m) => m.id === newestId.current);
      // Synced history is older than what the user saw arrive, so it is not "new".
      const arrived =
        previous < 0 ? 0 : messages.slice(previous + 1).filter((m) => m.syncedFrom === null).length;
      if (arrived > 0) setUnseen((count) => count + arrived);
    }
    newestId.current = newest;
    measureAnchor();
    // measureAnchor reads the same render's messages and refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [messages, listKey]);

  function jumpToNewest() {
    const element = scrollRef.current;
    if (!element) return;
    element.scrollTop = element.scrollHeight;
    atBottom.current = true;
    setAway(false);
    setUnseen(0);
  }

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
      ".chat-message__body img, .inline-image__img, .message-card__image img",
    )) {
      if (img === clickedImage) index = images.length;
      images.push({
        // Inline images show a blob: URL; the viewer saves from the data: URL behind it.
        src: originalSrc(img.currentSrc || img.src),
        url: img.dataset["imageUrl"],
        name: img.dataset["imageUrl"] ? imageName(img.dataset["imageUrl"]) : t("viewer.unnamed"),
        full: fullImageSource(img),
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
    if (event.key === "End" && event.ctrlKey && !event.defaultPrevented) {
      event.preventDefault();
      jumpToNewest();
      moveFocus(messages.length - 1);
      return;
    }
    if (event.key !== "Enter" && event.key !== " ") return;
    const image = messageImage(event.target);
    if (!image) return;
    event.preventDefault();
    event.stopPropagation();
    openViewerAt(image);
  }

  useEffect(measurePinned, [messages, display]);

  function onScroll() {
    const element = scrollRef.current;
    if (!element) return;
    atBottom.current = element.scrollHeight - element.scrollTop - element.clientHeight < EDGE_PX;
    setAway(!atBottom.current);
    if (atBottom.current) setUnseen(0);
    measureAnchor();
    measurePinned();
    if (element.scrollTop < EDGE_PX && canLoadOlder) loadOlder();
  }

  useEffect(() => {
    if (highlighted === null) return;
    const timer = setTimeout(() => {
      setHighlighted(null);
    }, HIGHLIGHT_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [highlighted]);

  function reply(index: number) {
    const message = messages[index];
    if (!message || !onReply) return;
    if (collapsedIds?.has(message.id) === true && !shownBlocked.has(message.id)) return;
    onReply(quoteFor(message, names[index] ?? "", imageLabel));
  }

  function jumpTo(index: number) {
    const item = itemRefs.current[index];
    const id = messages[index]?.id;
    if (!item || id === undefined) return;
    setFocusIndex(index);
    item.focus({ preventScroll: true });
    item.scrollIntoView({ block: "center" });
    setHighlighted(id);
  }

  useEffect(() => {
    if (copyNote === null) return;
    const timer = setTimeout(() => {
      setCopyNote(null);
    }, 2500);
    return () => {
      clearTimeout(timer);
    };
  }, [copyNote]);

  // Through the engine: the webview's own clipboard does not reach the desktop everywhere.
  function copyText(text: string, done: string | null, what: string) {
    getApi()
      .copyText(text)
      .then(
        () => {
          if (done !== null) setCopyNote(done);
        },
        (error: unknown) => {
          console.error(`Copying ${what} failed`, error);
          setCopyNote(t("copy.failed"));
        },
      );
  }

  const copySelectionItem = (selection: string) => ({
    label: t("copy.selection"),
    onSelect: () => {
      copyText(selection, t("copy.done"), "the selected text");
    },
  });

  // Ctrl+C: plain text with line breaks, without the hidden labels the browser would copy too.
  function onCopy(event: ClipboardEvent<HTMLDivElement>) {
    const text = selectedText(window.getSelection(), event.currentTarget);
    if (text === "") return;
    event.preventDefault();
    event.clipboardData.setData("text/plain", text);
    // Also through the engine, like the menu: the webview's copy may not reach the desktop.
    copyText(text, null, "the selected text");
  }

  /** The image menu for a right-click (or the menu key) on a chat image; false elsewhere. */
  function openImageMenu(event: ReactMouseEvent<HTMLLIElement>, selection: string): boolean {
    const target = event.target instanceof Element ? event.target : null;
    // Inline external images and message-card images (also while loading or failed), or an
    // image sent inside the message.
    const holder = target?.closest<HTMLElement>(".inline-image, .message-card__image") ?? null;
    const img = holder ? holder.querySelector("img") : target ? messageImage(target) : null;
    const element = img ?? holder;
    if (!element) return false;
    event.preventDefault();
    const url = element.dataset["imageUrl"] ?? null;
    const rect = element.getBoundingClientRect();
    const keyboard = event.clientX === 0 && event.clientY === 0;
    setImageMenu({
      // Cards show a blob: URL; copying and saving work on the data: URL behind it.
      src: img ? originalSrc(img.currentSrc || img.src) : null,
      url,
      name: url === null ? t("viewer.unnamed") : imageName(url),
      fileName: url === null ? null : imageName(url),
      full: (img ? fullImageSource(img)?.reference : undefined) ?? null,
      selection,
      x: keyboard ? rect.left : event.clientX,
      y: keyboard ? rect.bottom : event.clientY,
      opener: img?.closest("button") ?? img ?? holder?.querySelector("button") ?? null,
      image: img,
    });
    return true;
  }

  function openMenu(index: number, event: ReactMouseEvent<HTMLLIElement>) {
    // Mention chips and names have their own menus.
    if (event.defaultPrevented) return;
    const selection = selectedText(window.getSelection(), scrollRef.current);
    if (openImageMenu(event, selection)) return;
    // A link has its own menu; this also keeps the webview's default one away.
    const anchor = event.target instanceof Element ? event.target.closest("a[href]") : null;
    const url = anchor?.getAttribute("href")?.trim();
    if (anchor instanceof HTMLElement && url && SAFE_LINK.test(url)) {
      event.preventDefault();
      const rect = anchor.getBoundingClientRect();
      const keyboard = event.clientX === 0 && event.clientY === 0;
      setLinkMenu({
        url,
        selection,
        x: keyboard ? rect.left : event.clientX,
        y: keyboard ? rect.bottom : event.clientY,
        opener: anchor,
      });
      return;
    }
    // With a selection the menu leads with Copy (#114); the webview's own menu would drop it.
    if (!onReply && selection === "") return;
    event.preventDefault();
    const rect = event.currentTarget.getBoundingClientRect();
    const keyboard = event.clientX === 0 && event.clientY === 0;
    setMenu({
      index,
      selection,
      x: keyboard ? rect.left : event.clientX,
      y: keyboard ? rect.top : event.clientY,
      opener: itemRefs.current[index] ?? null,
    });
  }

  // The rows are memoised, so they get one stable set of actions that always calls the latest
  // handlers instead of closures that change on every render.
  const latest = useRef({ reply, openMenu, jumpTo, openViewer });
  useEffect(() => {
    latest.current = { reply, openMenu, jumpTo, openViewer };
  });

  // Another feature asked to show a message (notification centre, #143): jump once it is here.
  const reveal = usePendingReveal();
  useEffect(() => {
    if (reveal === null) return;
    const index = messages.findIndex((message) => message.id === reveal);
    if (index < 0) return;
    latest.current.jumpTo(index);
    revealDone(reveal);
  }, [reveal, messages]);
  const itemActions = useMemo<MessageItemActions>(
    () => ({
      setRef: (index, element) => {
        itemRefs.current[index] = element;
      },
      reply: (index) => {
        latest.current.reply(index);
      },
      openMenu: (index, event) => {
        latest.current.openMenu(index, event);
      },
      jumpTo: (index) => {
        latest.current.jumpTo(index);
      },
      onImageClick: (image, opener) => {
        latest.current.openViewer(image, opener);
      },
      showBlocked: (id) => {
        setShownBlocked((before) => new Set(before).add(id));
      },
    }),
    [],
  );

  function moveFocus(index: number) {
    const next = Math.max(0, Math.min(index, messages.length - 1));
    setFocusIndex(next);
    itemRefs.current[next]?.focus();
  }

  function onKeyDown(event: KeyboardEvent<HTMLOListElement>) {
    if (event.target !== itemRefs.current[active]) return;
    switch (event.key) {
      case "ArrowUp":
        if (active === 0 && canLoadOlder) loadOlder();
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
        jumpToNewest();
        break;
      case "r":
      case "R":
        if (!onReply || event.ctrlKey || event.altKey || event.metaKey) return;
        reply(active);
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  return (
    <>
      <div className="chat-list-frame">
        {/* Clicks on images sent inside messages open the viewer; keyboard users use Enter / Space. */}
        {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
        <div
          className="chat-list"
          ref={scrollRef}
          onScroll={onScroll}
          onLoadCapture={onMediaLoad}
          onClick={onListClick}
          onKeyDown={onListKeyDown}
          onCopy={onCopy}
        >
          {hidden > 0 || log.hasMore ? (
            <button
              type="button"
              className="chat-list__older"
              disabled={!canLoadOlder}
              onClick={loadOlder}
            >
              {hidden === 0 && log.loading ? t("list.loading") : t("list.loadOlder")}
            </button>
          ) : onSyncEarlier && log.loaded ? (
            <button
              type="button"
              className="chat-list__older"
              disabled={log.syncing}
              onClick={onSyncEarlier}
            >
              {t("list.loadEarlier")}
            </button>
          ) : null}
          <SyncStatus log={log} />
          {log.loadFailed ? (
            <p className="chat-list__notice" role="status">
              {t("list.loadFailed")}
            </p>
          ) : null}
          <LeadingPluginCards listKey={listKey} />
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
                  key={renderKey(message.id)}
                  message={message}
                  index={index}
                  outgoing={outgoingById.get(message.id) ?? null}
                  start={layout[index]?.start ?? true}
                  day={layout[index]?.newDay ? dayLabel(new Date(message.sentAt)) : null}
                  display={display}
                  avatar={display === "cozy" && layout[index]?.start ? avatarFor(message) : null}
                  time={time}
                  fullTime={fullTime}
                  tabIndex={index === active ? 0 : -1}
                  clickablePeople={clickablePeople}
                  linkPreviews={linkPreviews}
                  senderName={names[index] ?? message.senderName}
                  highlighted={highlighted === message.id}
                  canReply={onReply !== undefined}
                  original={originals[index] ?? null}
                  collapsed={
                    collapsedIds?.has(message.id) === true && !shownBlocked.has(message.id)
                  }
                  actions={itemActions}
                />
              ))}
            </ol>
          )}
        </div>
        {pinned !== null && messages[pinned] ? (
          <div className="chat-pinned" ref={bannerRef} aria-hidden="true">
            <Avatar
              name={names[pinned] ?? messages[pinned].senderName}
              src={avatarFor(messages[pinned])}
            />
            <span className="chat-pinned__name">
              {names[pinned] ?? messages[pinned].senderName}
            </span>
            <span className="chat-pinned__time">
              {time.format(new Date(messages[pinned].sentAt))}
            </span>
          </div>
        ) : null}
        {away ? (
          <button
            type="button"
            className="chat-jump"
            aria-label={
              unseen > 0 ? t("list.jumpToNewestUnseen", { count: unseen }) : t("list.jumpToNewest")
            }
            aria-keyshortcuts="Control+End"
            onClick={jumpToNewest}
          >
            {t("list.jumpToNewest")}
            {unseen > 0 ? (
              <span className="chat-jump__count" aria-hidden="true">
                {unseen}
              </span>
            ) : null}
          </button>
        ) : null}
      </div>
      {menu ? (
        <ContextMenu
          menu={menu}
          label={t("reply.menu", { name: names[menu.index] ?? "" })}
          onClose={() => {
            setMenu(null);
          }}
          items={[
            ...(menu.selection === "" ? [] : [copySelectionItem(menu.selection)]),
            ...(onReply
              ? [
                  {
                    label: t("reply.action"),
                    onSelect: () => {
                      reply(menu.index);
                    },
                  },
                ]
              : []),
          ]}
        />
      ) : null}
      {linkMenu ? (
        <ContextMenu
          menu={linkMenu}
          label={t("linkMenu.label")}
          onClose={() => {
            setLinkMenu(null);
          }}
          items={[
            ...(linkMenu.selection === "" ? [] : [copySelectionItem(linkMenu.selection)]),
            {
              label: t("linkMenu.copy"),
              onSelect: () => {
                copyText(linkMenu.url, t("linkMenu.copied"), "a link");
              },
            },
            {
              label: t("linkMenu.open"),
              onSelect: () => {
                openInBrowser(linkMenu.url);
              },
            },
          ]}
        />
      ) : null}
      {imageMenu ? (
        <ImageMenu
          menu={imageMenu}
          onClose={() => {
            setImageMenu(null);
          }}
          onOpenViewer={
            imageMenu.image
              ? () => {
                  if (imageMenu.image?.isConnected) openViewerAt(imageMenu.image);
                }
              : undefined
          }
          copyText={copyText}
          notify={setCopyNote}
        />
      ) : null}
      <p className="chat-link-copied" role="status" hidden={copyNote === null}>
        {copyNote ?? ""}
      </p>
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

/** Progress and result of a history sync with other Mumcord users (FR-1.60). */
function SyncStatus({ log }: { log: ConversationLog }) {
  const { t } = useTranslation("chat");
  let text: string | null = null;
  if (log.syncing) text = t("list.syncing");
  else if (log.syncFailed) text = t("list.syncFailed");
  else if (log.syncResult !== null)
    text = log.syncResult > 0 ? t("list.syncAdded", { count: log.syncResult }) : t("list.syncNone");
  return (
    <p className="chat-list__notice chat-list__sync" role="status" hidden={text === null}>
      {text}
    </p>
  );
}

/** What a row can do, by its position; one stable object so unchanged rows are not redrawn. */
interface MessageItemActions {
  setRef: (index: number, element: HTMLLIElement | null) => void;
  reply: (index: number) => void;
  openMenu: (index: number, event: ReactMouseEvent<HTMLLIElement>) => void;
  jumpTo: (index: number) => void;
  onImageClick: ImageClickHandler;
  showBlocked: (id: string) => void;
}

interface MessageItemProps {
  message: ChatMessage;
  index: number;
  start: boolean;
  /** Day separator label above this message, if it is the first of its day. */
  day: string | null;
  display: ChatDisplay;
  avatar: string | null;
  time: Intl.DateTimeFormat;
  fullTime: Intl.DateTimeFormat;
  tabIndex: number;
  clickablePeople: boolean;
  linkPreviews: boolean;
  /** The sender as shown (the server's name for server messages). */
  senderName: string;
  /** The original of a reply that was just jumped to. */
  highlighted: boolean;
  canReply: boolean;
  /** Position of the reply's original in the loaded messages, if it is loaded. */
  original: number | null;
  actions: MessageItemActions;
  /** Set while the own message is not confirmed: sending, or failed with a reason. */
  outgoing: OutgoingSend | null;
  /** From a blocked user and not shown yet: only a placeholder with "Show" (issue #140). */
  collapsed: boolean;
}

const MessageItem = memo(function MessageItem({
  message,
  index,
  outgoing,
  start,
  day,
  display,
  avatar,
  time,
  fullTime,
  tabIndex,
  clickablePeople,
  linkPreviews,
  senderName,
  highlighted,
  canReply,
  original,
  collapsed,
  actions,
}: MessageItemProps) {
  const t = useT("chat");
  const { onImageClick } = actions;
  const itemRef = useCallback(
    (element: HTMLLIElement | null) => {
      actions.setRef(index, element);
    },
    [actions, index],
  );
  const onReply =
    canReply && !collapsed
      ? () => {
          actions.reply(index);
        }
      : null;
  const onContextMenu = (event: ReactMouseEvent<HTMLLIElement>) => {
    // A collapsed message offers nothing that would show or copy its text.
    if (collapsed) return;
    actions.openMenu(index, event);
  };
  const onJump =
    original === null
      ? null
      : () => {
          actions.jumpTo(original);
        };
  const mentioned = useIsMention(message) && !collapsed;
  const sentAt = new Date(message.sentAt);
  const session = message.senderSession;
  const fromServer = message.senderName === "" && session === null && !message.own;
  // Avatar and name of every sender open the preview and the context menu. Someone who left (or
  // history without a session) still gets them, limited to the stored name.
  const preview = (content: ReactNode) => (
    <ProfilePreviewTrigger session={session} name={senderName}>
      {content}
    </ProfilePreviewTrigger>
  );
  const senderUser = useServerSelector((state) =>
    session === null ? undefined : state.users.get(session),
  );
  const sender = fromServer ? (
    <span className="chat-message__sender chat-message__sender--server">
      <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true" focusable="false">
        <path
          d="M4 5h16v6H4zM4 13h16v6H4zM8 8h.01M8 16h.01"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
        />
      </svg>{" "}
      {senderName}
    </span>
  ) : clickablePeople && session !== null ? (
    preview(
      <button
        type="button"
        className="chat-message__sender chat-message__sender--button"
        tabIndex={tabIndex}
        onClick={(event) => {
          clickPerson(event.detail, session, event.currentTarget, senderName);
        }}
        onDoubleClick={() => {
          doubleClickPerson(session, senderName);
        }}
      >
        {senderName}
        <MumcordBadge user={senderUser} />
      </button>,
    )
  ) : (
    preview(
      <span className="chat-message__sender">
        {senderName}
        <MumcordBadge user={senderUser} />
      </span>,
    )
  );
  const origin =
    message.syncedFrom !== null ? (
      <span className="chat-message__origin">
        {t("list.syncedFrom", { name: message.syncedFrom })}
      </span>
    ) : null;
  const stamp = (className: string) => (
    <time className={className} dateTime={message.sentAt} data-tooltip={fullTime.format(sentAt)}>
      {time.format(sentAt)}
    </time>
  );

  let row;
  if (collapsed) {
    // Nothing of the message itself, not even the name: only that one is hidden here.
    row = (
      <div className="chat-message__row">
        {display === "cozy" && (
          <div className="chat-message__gutter">
            {start ? <Avatar name={senderName} src={avatar} /> : null}
          </div>
        )}
        <div className="chat-message__content">
          <CollapsedMessage
            tabIndex={tabIndex}
            onShow={() => {
              actions.showBlocked(message.id);
            }}
          />
        </div>
      </div>
    );
  } else if (display === "compact") {
    row = (
      <div className="chat-message__row">
        {stamp("chat-message__time")} {sender}{" "}
        <Body
          message={message}
          tabIndex={tabIndex}
          previews={linkPreviews}
          onImageClick={onImageClick}
          clickablePeople={clickablePeople}
          onJump={onJump}
        />
        {origin}
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
                  clickPerson(event.detail, session, event.currentTarget, senderName);
                }}
                onDoubleClick={() => {
                  doubleClickPerson(session, senderName);
                }}
              >
                <Avatar name={senderName} src={avatar} />
              </span>
            ) : (
              <Avatar name={senderName} src={avatar} />
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
            onJump={onJump}
          />
          {origin}
        </div>
      </div>
    );
  } else {
    // Grouped: the sender stays readable for screen readers; the time shows on hover/focus.
    row = (
      <div className="chat-message__row">
        <span className="chat-message__sender visually-hidden">{senderName} </span>
        <div className="chat-message__gutter">{stamp("chat-message__hover-time")}</div>
        <div className="chat-message__content">
          <Body
            message={message}
            tabIndex={tabIndex}
            previews={linkPreviews}
            onImageClick={onImageClick}
            clickablePeople={clickablePeople}
            onJump={onJump}
          />
          {origin}
        </div>
      </div>
    );
  }

  return (
    // Roving tab stop so screen-reader and keyboard users can step through messages; the
    // context menu is also reachable with the menu key, and R or the button replies.
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions
    <li
      ref={itemRef}
      className={
        (start ? "chat-message chat-message--start" : "chat-message") +
        (mentioned ? " chat-message--mention" : "") +
        (highlighted ? " chat-message--highlight" : "") +
        (outgoing ? " chat-message--pending" : "")
      }
      tabIndex={tabIndex}
      aria-keyshortcuts={onReply ? "R" : undefined}
      onContextMenu={onContextMenu}
    >
      {mentioned ? <span className="visually-hidden">{t("mention.mentionsYou")}. </span> : null}
      {day !== null ? <div className="chat-day">{day}</div> : null}
      {row}
      {outgoing?.error != null ? <SendFailure entry={outgoing} reason={outgoing.error} /> : null}
      {onReply ? (
        <div className="chat-message__actions">
          <button
            type="button"
            className="chat-message__action"
            tabIndex={tabIndex}
            aria-label={t("reply.actionFor", { name: senderName })}
            data-tooltip={t("reply.action")}
            onClick={onReply}
          >
            <ReplyIcon size={16} />
          </button>
        </div>
      ) : null}
    </li>
  );
});
