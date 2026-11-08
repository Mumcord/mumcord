/**
 * Message input (issue #596): one compact bar with the text field on top and the icon row below.
 * The bar starts on a single line, grows with the text up to about six lines and scrolls from
 * there; the formatting toolbar is a pill on its top-left edge while the cursor is inside and the
 * bar carries the only focus ring. The send button is muted until there is anything to send, and
 * without the text-message permission the bar becomes a locked note instead of an input.
 *
 * Markdown with shortcuts, Enter to send (Shift+Enter for a new line), images (one or several) by
 * button, paste or drop into an attachment tray, and inline errors such as a denied permission or
 * a too-large image. While replying (FR-2.60) a preview of the quoted message sits above the bar;
 * × or Escape cancels it.
 */
import {
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ClipboardEvent,
  type DragEvent,
  type KeyboardEvent,
} from "react";
import { useTranslation } from "react-i18next";
import { ComposerIcon } from "../../components/ComposerIcon";
import type { Conversation } from "../../api";
import { isFileDrag } from "../../app/nativeDrag";
import { selfUser, useServerSelector } from "../channels";
import { useNoRelayHint } from "../files/composer/noRelayHint";
import { RefusalAction } from "../p2p/RefusalAction";
import { useGuestRelayBlock } from "../p2p/useGuestRelayBlock";
import { needsDecision, openRelayDetails } from "../p2p/relayTrust";
import { useRelayInfo } from "../p2p/useRelayInfo";
import { useServers } from "../servers";
import { useAvatars } from "../members/useAvatars";
import type { SendTarget } from "./conversationInfo";
import { ChatActionRow } from "./ChatActionRow";
import { AttachmentTray } from "./AttachmentTray";
import { hasNativeFileDrop, isOverRect, onNativeFileDrop } from "./fileDrop";
import { FormatToolbar } from "./FormatToolbar";
import { GifPicker } from "./GifPicker";
import { applyFormat, formatForKey, type Edit, type Format } from "./formatting";
import type { MessageLimits } from "./images";
import {
  findMentionQuery,
  insertMention,
  optionId,
  rankGroupMentions,
  rankMentionCandidates,
  type MentionQuery,
} from "./mentions";
import { MentionPicker, type MentionOption } from "./MentionPicker";
import { SlashPicker } from "./SlashPicker";
import { commandOptionId, useSlashCommands } from "./slashCommands";
import { useAdminSlash } from "../admin/commands/useAdminSlash";
import type { ReplyQuote } from "./reply";
import { credentialLinks as countCredentials, mumbleLinks } from "./mumbleLink";
import { MumbleLinkCard } from "./MumbleLinkCard";
import { useComposer } from "./useComposer";
import { FileChipTray } from "../files/composer/FileChipTray";
import { DropChoice } from "../files/composer/DropChoice";
import { splitDropped } from "../files/composer/dropSplit";
import { FileDropOverlay } from "../files/composer/FileDropOverlay";
import { PaperclipButton } from "../files/composer/PaperclipButton";
import { unavailableText } from "../files/composer/unavailableText";
import { useFileChips } from "../files/composer/useFileChips";
import { useFileDrop } from "../files/composer/useFileDrop";
import { isHiddenBot } from "../channels/mumcord";

export interface ComposerProps {
  target: SendTarget | null;
  /** The conversation being typed in; a command's result line lands here (issue #378). */
  conversation: Conversation;
  limits: MessageLimits;
  /** Accessible name and placeholder, e.g. "Message #Lobby". */
  label: string;
  /** Why writing is impossible right now (no permission, person offline); disables the input. */
  blockedReason: string | null;
  /** The link-previews privacy switch: off means the GIF picker loads nothing from the web. */
  linkPreviews?: boolean;
  /** The message being replied to, shown as a preview and quoted in the sent message. */
  reply?: ReplyQuote | null;
  onCancelReply?: () => void;
}

// Up arrow, as in the mockup: sending is a plain "send", not a paper plane.
const IMAGE_ICON = "M4 5h16v14H4zM4 16l5-5 4 4 3-3 4 4M15 9h.01";
const SEND_ICON = "M12 19V5M5 12l7-7 7 7";
const CANCEL_ICON = "M6 6l12 12M18 6L6 18";
const LOCK_ICON = ["M5 11h14v9H5z", "M8 11V8a4 4 0 0 1 8 0v3"];

/** `DataTransferItemList` is a list, not an iterable, so it is read by index. */
function clipboardItems(data: DataTransfer): DataTransferItem[] {
  const items = data.items;
  const out: DataTransferItem[] = [];
  for (let i = 0; i < items.length; i += 1) {
    const item = items[i];
    if (item !== undefined) out.push(item);
  }
  return out;
}

/**
 * Files of a paste. WebKitGTK does not fill `clipboardData.files` for a clipboard image (issue
 * #50): a screenshot arrives as an item only, so reading `files` alone makes the paste do nothing.
 * It sometimes hands over a clipboard image as an empty file, which counts as not handed over
 * (issue #110).
 */
function pasteFiles(data: DataTransfer): File[] {
  const files = Array.from(data.files);
  const handed =
    files.length > 0
      ? files
      : clipboardItems(data)
          .filter((item) => item.kind === "file")
          .map((item) => item.getAsFile())
          .filter((file): file is File => file !== null);
  return handed.filter((file) => file.size > 0);
}

/**
 * Whether a paste holds files copied in the file manager that the webview keeps to itself: WebKitGTK
 * lists `text/uri-list` for them but gives neither `File` objects nor the paths (issue #50). A
 * copied web link also comes as `text/uri-list`, but its URLs stay readable.
 */
function hasHiddenFiles(data: DataTransfer): boolean {
  if (!data.types.includes("text/uri-list")) return false;
  return data
    .getData("text/uri-list")
    .split(/\r?\n/)
    .every((line) => line.trim() === "" || line.startsWith("#") || line.startsWith("file:"));
}

/**
 * Whether a paste holds text the input takes as it is. A paste without it may still hold copied
 * image data the webview did not hand over (issue #110).
 */
function hasText(data: DataTransfer): boolean {
  return data.types.includes("text/plain") || data.types.includes("text/uri-list");
}

export function Composer({
  target,
  conversation,
  limits,
  label,
  blockedReason,
  linkPreviews = false,
  reply = null,
  onCancelReply,
}: ComposerProps) {
  const { t } = useTranslation("chat");
  const { t: tFiles } = useTranslation("files");
  const { t: tP2p } = useTranslation("p2p");
  const composer = useComposer(target, limits, reply, onCancelReply);
  const inChannelTarget = target?.kind === "channel";
  const fileChips = useFileChips(inChannelTarget);
  const serverId = useServerSelector((state) => state.serverId);
  const { servers } = useServers();
  const server = servers.find((entry) => entry.id === serverId);
  const serverName = server ? server.label || server.address : "";
  const relayInfo = useRelayInfo(inChannelTarget ? serverId : null);
  // A changed relay that waits for trust blocks files like a missing one, but says so (#600).
  const relayWaiting =
    fileChips.unavailable?.kind === "noRelay" && relayInfo !== null && needsDecision(relayInfo);
  const noRelayHint = useNoRelayHint(
    serverId,
    fileChips.unavailable === null
      ? false
      : fileChips.unavailable.kind === "noRelay" && !relayWaiting,
  );
  // A guest on a relay for registered users only is told before trying (#601).
  const guestBlocked = useGuestRelayBlock(inChannelTarget ? serverId : null);
  // Only the plain "no relay" hint can be closed (#579); the others ask for an action.
  const reasonShown =
    inChannelTarget &&
    ((fileChips.unavailable === null && guestBlocked) ||
      (fileChips.unavailable !== null &&
        (relayWaiting || !(fileChips.unavailable.kind === "noRelay" && noRelayHint.hidden))));
  const reasonText =
    fileChips.unavailable !== null
      ? relayWaiting
        ? tP2p("trust.filesBlocked")
        : unavailableText(tP2p, fileChips.unavailable)
      : guestBlocked
        ? tP2p("refusalAction.filesRegisteredOnly")
        : null;
  const reasonId = useId();
  // A `mumble://` link the user typed is shown as its card before sending (issue #175).
  const previewLinks = useMemo(() => mumbleLinks(composer.text), [composer.text]);
  const credentialLinks = useMemo(() => countCredentials(previewLinks), [previewLinks]);
  const fileInput = useRef<HTMLInputElement>(null);
  const formRef = useRef<HTMLFormElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  // Selection to restore once a formatting edit has been rendered into the textarea.
  const pendingSelection = useRef<Edit | null>(null);
  const hintId = useId();
  const errorId = useId();
  const pickerId = useId();
  const commandListId = `${pickerId}-commands`;
  const disabled = blockedReason !== null;
  // Mumcord's own admin commands, then the plugins' (FR-10.30, FR-6.43): a typed command goes to
  // the engine or its plugin, never into the chat.
  const admin = useAdminSlash(composer.text, composer.setText, disabled, conversation);
  const slash = useSlashCommands(composer.text, composer.setText, disabled, admin);

  // Mention autocomplete (FR-1.51).
  const [caret, setCaret] = useState(0);
  const [active, setActive] = useState(0);
  // The `@query` whose list was closed (Escape or a pick); it stays closed while the query still
  // begins with that text, so a picked "@Ann " does not reopen for the longer name "Ann Lee".
  const [dismissed, setDismissed] = useState<MentionQuery | null>(null);
  const users = useServerSelector((state) => state.users);
  const selfSession = useServerSelector((state) => state.selfSession);
  const ownChannel = useServerSelector((state) => selfUser(state)?.channelId ?? null);
  const mention = useMemo(
    // A slash line is a command; its own picker offers the people or channels an argument takes,
    // so the @ mention list must not open on top of it (issue #378).
    () =>
      disabled || composer.text.startsWith("/") ? null : findMentionQuery(composer.text, caret),
    [composer.text, caret, disabled],
  );
  const hereChannel = target?.kind === "channel" ? target.channelId : ownChannel;
  // `@channel` and `@all` only make sense in a channel (issue #70).
  const inChannel = target?.kind === "channel";
  const hereName = useServerSelector((state) =>
    hereChannel === null ? "" : (state.channels.get(hereChannel)?.name ?? ""),
  );
  // Who `@channel` and `@all` reach: the people in the channel, and those plus its listeners.
  const reach = useMemo(() => {
    let members = 0;
    let readers = 0;
    for (const user of users.values()) {
      if (isHiddenBot(user)) continue;
      const member = user.channelId === hereChannel;
      if (member) members += 1;
      if (member || (hereChannel !== null && user.listeningChannelIds.includes(hereChannel))) {
        readers += 1;
      }
    }
    return { members, readers };
  }, [users, hereChannel]);
  const candidates = useMemo(
    () =>
      mention === null
        ? []
        : rankMentionCandidates(users.values(), mention.query, hereChannel, selfSession),
    [mention, users, hereChannel, selfSession],
  );
  const options = useMemo<MentionOption[]>(
    () => [
      ...(mention === null || !inChannel ? [] : rankGroupMentions(mention.query)).map((group) => ({
        kind: "group" as const,
        group,
      })),
      ...candidates.map((user) => ({ kind: "user" as const, user })),
    ],
    [mention, inChannel, candidates],
  );
  const isDismissed =
    mention !== null &&
    dismissed !== null &&
    mention.start === dismissed.start &&
    mention.query.startsWith(dismissed.query);
  const pickerOpen = mention !== null && !isDismissed && options.length > 0;
  const activeIndex = Math.min(active, Math.max(options.length - 1, 0));
  const avatarOf = useAvatars(pickerOpen ? candidates : []);

  // Counts nested dragenter/dragleave pairs so the highlight does not flicker over child elements.
  const dragDepth = useRef(0);
  const [dragging, setDragging] = useState(false);
  // Native file drags (desktop app) over the composer; they bypass the HTML5 drag events.
  const [nativeOver, setNativeOver] = useState(false);
  // In a channel where sharing works, every drop on the chat becomes a file chip instead.
  const shareDrops = inChannelTarget && fileChips.unavailable === null && !disabled;
  const [chatHost, setChatHost] = useState<HTMLElement | null>(null);
  useLayoutEffect(() => {
    setChatHost(formRef.current?.closest<HTMLElement>(".chat") ?? null);
  }, []);
  const [pendingDrop, setPendingDrop] = useState<readonly string[] | null>(null);
  const dropOver = useFileDrop({
    host: chatHost,
    accept: shareDrops,
    rejectOutside: target?.kind === "private" && !disabled,
    exclude: shareDrops ? null : formRef,
    onPaths: (paths) => {
      const { images, others } = splitDropped(paths);
      // Images may mean an inline picture or a file to share: ask. Anything else is a file.
      if (images.length > 0) {
        setPendingDrop(paths);
        return;
      }
      void fileChips.addPaths(others);
    },
    onRejected: () => {
      fileChips.fail(tFiles("composer.privateChat"));
    },
  });
  const nativeDrop = useRef({
    disabled: disabled || shareDrops,
    addDroppedPaths: composer.addDroppedPaths,
  });
  useEffect(() => {
    nativeDrop.current = {
      disabled: disabled || shareDrops,
      addDroppedPaths: composer.addDroppedPaths,
    };
  });

  useEffect(() => {
    if (!hasNativeFileDrop()) return;
    return onNativeFileDrop((event) => {
      if (event.type === "leave") {
        setNativeOver(false);
        return;
      }
      const form = formRef.current;
      const over =
        form !== null &&
        !nativeDrop.current.disabled &&
        isOverRect(event.position, form.getBoundingClientRect(), window.devicePixelRatio);
      if (event.type === "drop") {
        setNativeOver(false);
        dragDepth.current = 0;
        setDragging(false);
        if (over) void nativeDrop.current.addDroppedPaths(event.paths);
        return;
      }
      setNativeOver(over);
    });
  }, []);

  useLayoutEffect(() => {
    const pending = pendingSelection.current;
    const element = input.current;
    if (pending === null || element === null || element.value !== pending.text) return;
    pendingSelection.current = null;
    element.focus();
    element.setSelectionRange(pending.selectionStart, pending.selectionEnd);
  }, [composer.text]);

  // One line when empty, growing with the text; chat.css caps the height, from where the text
  // scrolls (issue #596). Measuring after every text change keeps the caret in view. `pillShown`
  // mirrors the CSS rule that shows the formatting pill: while it is over the bar, the field has
  // more top padding and its height has to be measured again.
  const [pillShown, setPillShown] = useState(false);
  useLayoutEffect(() => {
    const element = input.current;
    if (element === null) return;
    element.style.height = "auto";
    // jsdom reports no layout at all, so there a measured height of 0 would shrink the field.
    if (element.scrollHeight > 0) element.style.height = `${String(element.scrollHeight)}px`;
  }, [composer.text, pillShown]);

  /** Sends the typed text and images, then one offer per ready file chip. */
  function submitAll() {
    if (fileChips.busy) return;
    composer.submit();
    if (target?.kind === "channel" && fileChips.hasSendable) void fileChips.send(target.channelId);
  }

  // The send button shows whether there is anything to send (issue #596): muted while the input
  // is empty and no picture or file is ready, and while one is still worked on (sending then does
  // nothing, so an enabled button would only lead to a dead Enter).
  const waiting =
    target === null ||
    fileChips.busy ||
    composer.attachments.some((item) => item.status === "processing");
  const hasReadyAttachment = composer.attachments.length > 0 || fileChips.hasSendable;
  const canSend = !waiting && (composer.text.trim() !== "" || hasReadyAttachment);
  // With a picture or a file already in the tray, the typed text is only a comment on it.
  const placeholder =
    composer.text === "" && hasReadyAttachment ? t("composer.optionalMessage") : label;

  function format(kind: Format) {
    const element = input.current;
    if (element === null) return;
    const edit = applyFormat(
      {
        text: element.value,
        selectionStart: element.selectionStart,
        selectionEnd: element.selectionEnd,
      },
      kind,
    );
    pendingSelection.current = edit;
    composer.setText(edit.text);
  }

  function pick(option: MentionOption) {
    const element = input.current;
    if (mention === null || element === null) return;
    const name = option.kind === "group" ? option.group : option.user.name;
    const edit = insertMention(element.value, mention.start, element.selectionStart, name);
    pendingSelection.current = {
      text: edit.text,
      selectionStart: edit.caret,
      selectionEnd: edit.caret,
    };
    setDismissed({ start: mention.start, query: `${name} ` });
    setCaret(edit.caret);
    composer.setText(edit.text);
  }

  /** Handles the keys of the open mention list; true when the key was used. */
  function onPickerKey(event: KeyboardEvent<HTMLTextAreaElement>): boolean {
    if (!pickerOpen || event.nativeEvent.isComposing) return false;
    const count = options.length;
    switch (event.key) {
      case "ArrowDown":
        setActive((activeIndex + 1) % count);
        break;
      case "ArrowUp":
        setActive((activeIndex + count - 1) % count);
        break;
      case "Enter":
      case "Tab": {
        const option = options[activeIndex];
        if (option === undefined || event.shiftKey || event.ctrlKey || event.altKey) return false;
        pick(option);
        break;
      }
      case "Escape":
        setDismissed(mention);
        break;
      default:
        return false;
    }
    event.preventDefault();
    event.stopPropagation();
    return true;
  }

  // Choosing "Reply" moves straight to the input.
  useEffect(() => {
    if (reply !== null) input.current?.focus();
  }, [reply]);

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (slash.onKey(event)) return;
    if (onPickerKey(event)) return;
    if (event.key === "Escape" && reply !== null && onCancelReply) {
      event.preventDefault();
      onCancelReply();
      return;
    }
    if (
      event.key === "Backspace" &&
      composer.text === "" &&
      composer.attachments.length > 0 &&
      !event.nativeEvent.isComposing
    ) {
      event.preventDefault();
      const last = composer.attachments[composer.attachments.length - 1];
      if (last) composer.removeAttachment(last.id);
      return;
    }
    const shortcut = formatForKey(event);
    if (shortcut !== null) {
      // Also tells global shortcuts (Ctrl+K channel search) that the key was handled here.
      event.preventDefault();
      format(shortcut);
      return;
    }
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      if (!slash.run(composer.text)) submitAll();
    }
  }

  function onPaste(event: ClipboardEvent<HTMLTextAreaElement>) {
    const data = event.clipboardData;
    const files = pasteFiles(data);
    if (files.length === 0) {
      // Image data in the paste always wins over a file reference, so it can never come here.
      const imageListed = clipboardItems(data).some(
        (item) => item.kind === "file" && item.type.startsWith("image/"),
      );
      if (imageListed || hasHiddenFiles(data)) {
        event.preventDefault();
        void composer.addFromClipboard(imageListed);
        return;
      }
      if (!hasText(data)) {
        // WebKitGTK lists no types at all for text offered only as `text/plain;charset=utf-8`, so
        // the webview must insert it. Only when it inserted nothing may it have been image data
        // the webview did not hand over (issue #110).
        const input = event.currentTarget;
        const before = input.value;
        window.setTimeout(() => {
          if (input.value === before) void composer.addFromClipboard(false);
        }, 0);
      }
      return;
    }
    // Text copied together with a file (e.g. from a spreadsheet) still pastes as text.
    if (!files.some((file) => file.type.startsWith("image/"))) return;
    event.preventDefault();
    composer.addFiles(files);
  }

  function hasFiles(event: DragEvent): boolean {
    return isFileDrag(event.dataTransfer);
  }

  function onDragEnter(event: DragEvent) {
    if (disabled || !hasFiles(event)) return;
    dragDepth.current += 1;
    setDragging(true);
  }

  function onDragLeave(event: DragEvent) {
    if (!hasFiles(event)) return;
    dragDepth.current = Math.max(0, dragDepth.current - 1);
    if (dragDepth.current === 0) setDragging(false);
  }

  function onDragOver(event: DragEvent) {
    // Marks the composer as a drop target; without it the drop never reaches onDrop.
    if (!disabled && hasFiles(event)) event.preventDefault();
  }

  function onDrop(event: DragEvent) {
    if (!hasFiles(event)) return;
    // Also keeps the textarea from inserting the dropped `file://` link as text.
    event.preventDefault();
    dragDepth.current = 0;
    setDragging(false);
    // In the desktop app the native drop event delivers the files; reading them here too
    // would attach every image twice.
    if (!disabled && !hasNativeFileDrop()) composer.addFiles(Array.from(event.dataTransfer.files));
  }

  return (
    <>
      {/* Dragging only marks the composer as a drop target; the keyboard path is the image button. */}
      {/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions */}
      <form
        ref={formRef}
        className="chat-composer"
        data-dragging={dragging || nativeOver ? "true" : undefined}
        onDragEnter={onDragEnter}
        onDragLeave={onDragLeave}
        onDragOver={onDragOver}
        onDrop={onDrop}
        onSubmit={(event) => {
          event.preventDefault();
          if (!slash.run(composer.text)) submitAll();
        }}
      >
        {reasonShown && reasonText !== null ? (
          <div className="chat-composer__hint">
            <p id={reasonId} className="chat-composer__blocked">
              {reasonText}
              {fileChips.unavailable === null && serverId !== null && (
                <>
                  {" "}
                  <RefusalAction hint="register" serverId={serverId} />
                </>
              )}
              {relayWaiting && serverId !== null && (
                <>
                  {" "}
                  <button
                    type="button"
                    className="chat-composer__hint-link"
                    onClick={() => {
                      openRelayDetails(serverId);
                    }}
                  >
                    {tP2p("trust.review")}
                  </button>
                </>
              )}
            </p>
            {fileChips.unavailable?.kind === "noRelay" && !relayWaiting && (
              <button
                type="button"
                className="chat-composer__hint-close"
                aria-label={tP2p("relay.hideNoRelay", { server: serverName })}
                data-tooltip={tP2p("relay.hideNoRelay", { server: serverName })}
                onClick={noRelayHint.hide}
              >
                <span aria-hidden="true">×</span>
              </button>
            )}
          </div>
        ) : null}
        {fileChips.error ? (
          <p className="chat-composer__error" role="alert">
            {fileChips.error}
          </p>
        ) : null}
        {slash.error ? (
          <p className="chat-composer__error" role="alert">
            {slash.error}
          </p>
        ) : null}
        {admin.notice ? (
          // Why a command did not run: no such person, or a name several people match (FR-10.34).
          <p className="chat-composer__error" role="alert">
            {admin.notice}
          </p>
        ) : null}
        {composer.error ? (
          <p id={errorId} className="chat-composer__error" role="alert">
            {composer.error}
          </p>
        ) : null}
        {reply !== null ? (
          <div className="chat-composer__reply">
            <span className="chat-composer__reply-text">
              {t("reply.replyingTo")} <span className="chat-reply__name">{reply.name}</span>
              {": "}
              {reply.snippet}
            </span>
            {onCancelReply ? (
              <button
                type="button"
                className="chat-composer__reply-cancel"
                aria-label={t("reply.cancel")}
                data-tooltip={t("reply.cancel")}
                onClick={() => {
                  onCancelReply();
                  input.current?.focus();
                }}
              >
                <ComposerIcon d={CANCEL_ICON} size={18} />
              </button>
            ) : null}
          </div>
        ) : null}
        {previewLinks.length > 0 ? (
          <div className="chat-composer__links">
            {previewLinks.map((url) => (
              <MumbleLinkCard
                key={url}
                url={url}
                credentialLinks={credentialLinks}
                preview
                tabIndex={0}
              />
            ))}
          </div>
        ) : null}
        <FileChipTray chips={fileChips.chips} onRemove={fileChips.remove} />
        <AttachmentTray items={composer.attachments} onRemove={composer.removeAttachment} />
        {pickerOpen ? (
          <MentionPicker
            id={pickerId}
            options={options}
            active={activeIndex}
            avatarOf={avatarOf}
            channel={{ name: hereName, members: reach.members, readers: reach.readers }}
            onPick={pick}
          />
        ) : null}
        <SlashPicker id={commandListId} slash={slash} />
        <p className="visually-hidden" role="status">
          {pickerOpen
            ? t("mention.found", { count: options.length })
            : slash.open
              ? t("mention.found", { count: slash.options.length })
              : ""}
        </p>
        {blockedReason === null ? (
          // The handlers only watch where the focus is, so the field can be measured again once
          // the pill's padding appears; the div itself is not interactive.
          // eslint-disable-next-line jsx-a11y/no-static-element-interactions
          <div
            className="chat-composer__bar"
            onFocus={() => {
              setPillShown(true);
            }}
            onBlur={(event) => {
              const to = event.relatedTarget;
              if (!(to instanceof Node) || !event.currentTarget.contains(to)) setPillShown(false);
            }}
          >
            <FormatToolbar onFormat={format} />
            <textarea
              ref={input}
              // A textarea cannot be a combobox (axe, issue #627): it stays a textbox that names
              // the open list and its highlighted row, and the status line above counts the rows.
              aria-controls={pickerOpen ? pickerId : slash.open ? commandListId : undefined}
              aria-activedescendant={
                pickerOpen
                  ? optionId(pickerId, activeIndex)
                  : slash.open
                    ? commandOptionId(commandListId, slash.active)
                    : undefined
              }
              aria-autocomplete={pickerOpen || slash.open ? "list" : undefined}
              className="chat-composer__input"
              rows={1}
              aria-label={label}
              aria-describedby={composer.error ? `${hintId} ${errorId}` : hintId}
              placeholder={placeholder}
              value={composer.text}
              onChange={(event) => {
                composer.setText(event.target.value);
                setCaret(event.target.selectionStart);
                setActive(0);
              }}
              onSelect={(event) => {
                setCaret(event.currentTarget.selectionStart);
              }}
              onKeyDown={onKeyDown}
              onPaste={onPaste}
            />
            <div className="chat-composer__tools">
              <ChatActionRow />
              {inChannelTarget ? (
                <PaperclipButton
                  disabled={disabled}
                  reason={reasonText}
                  describedBy={reasonShown ? reasonId : undefined}
                  onPick={(paths) => {
                    void fileChips.addPaths(paths);
                  }}
                />
              ) : null}
              <button
                type="button"
                className="chat-composer__icon"
                aria-label={t("composer.addImage")}
                data-tooltip={t("composer.addImage")}
                disabled={disabled}
                onClick={() => fileInput.current?.click()}
              >
                <ComposerIcon d={IMAGE_ICON} />
              </button>
              <GifPicker
                onSend={composer.sendLink}
                disabled={disabled}
                linkPreviews={linkPreviews}
              />
              <input
                ref={fileInput}
                type="file"
                accept="image/png,image/jpeg,image/gif,image/webp"
                hidden
                multiple
                onChange={(event) => {
                  composer.addFiles(Array.from(event.currentTarget.files ?? []));
                  event.currentTarget.value = "";
                }}
              />
              <button
                type="submit"
                className="chat-composer__send"
                aria-label={t("composer.send")}
                // Stays focusable when there is nothing to send, so its state can be discovered.
                aria-disabled={canSend ? undefined : true}
              >
                <ComposerIcon d={SEND_ICON} size={16} />
                <span className="chat-composer__send-label">{t("composer.send")}</span>
              </button>
            </div>
          </div>
        ) : (
          // Without the text-message permission there is no field to disable but a note: the
          // bar turns into a locked line. Starting a share still works by voice, so the Apps
          // button keeps its row (issue #596).
          <div className="chat-composer__bar chat-composer__bar--locked">
            <p className="chat-composer__locked">
              <ComposerIcon d={LOCK_ICON} />
              {blockedReason}
            </p>
            <div className="chat-composer__tools">{inChannelTarget ? <ChatActionRow /> : null}</div>
          </div>
        )}
        <p id={hintId} className="visually-hidden">
          {t("composer.hint")}
        </p>
        {pendingDrop !== null ? (
          <DropChoice
            onImage={() => {
              const { images, others } = splitDropped(pendingDrop);
              setPendingDrop(null);
              void composer.addDroppedPaths(images);
              if (others.length > 0) void fileChips.addPaths(others);
              input.current?.focus();
            }}
            onFile={() => {
              setPendingDrop(null);
              void fileChips.addPaths(pendingDrop);
              input.current?.focus();
            }}
            onCancel={() => {
              setPendingDrop(null);
              input.current?.focus();
            }}
          />
        ) : null}
        {dropOver && chatHost !== null ? (
          <FileDropOverlay host={chatHost} channel={hereName} />
        ) : null}
      </form>
    </>
  );
}
