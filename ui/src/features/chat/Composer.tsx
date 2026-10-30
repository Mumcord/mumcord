/**
 * Message input: Markdown with a formatting toolbar and shortcuts, Enter to send (Shift+Enter for a new line), images (one or
 * several) by button, paste or drop into an attachment tray, and inline errors such as a denied permission or a too-large image.
 * While replying (FR-2.60) a preview of the quoted message sits above the input; × or Escape cancels it.
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
import type { User } from "../../api";
import { isFileDrag } from "../../app/nativeDrag";
import { selfUser, useServerSelector } from "../channels";
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
  rankMentionCandidates,
  type MentionQuery,
} from "./mentions";
import { MentionPicker } from "./MentionPicker";
import type { ReplyQuote } from "./reply";
import { credentialLinks as countCredentials, mumbleLinks } from "./mumbleLink";
import { MumbleLinkCard } from "./MumbleLinkCard";
import { useComposer } from "./useComposer";

export interface ComposerProps {
  target: SendTarget | null;
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

function Icon({ d }: { d: string }) {
  return (
    <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" focusable="false">
      <path d={d} fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}

const IMAGE_ICON = "M4 5h16v14H4zM4 16l5-5 4 4 3-3 4 4M15 9h.01";
const SEND_ICON = "M4 12l16-8-6 16-2-7z";
const CANCEL_ICON = "M6 6l12 12M18 6L6 18";

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
  limits,
  label,
  blockedReason,
  linkPreviews = false,
  reply = null,
  onCancelReply,
}: ComposerProps) {
  const { t } = useTranslation("chat");
  const composer = useComposer(target, limits, reply, onCancelReply);
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
  const disabled = blockedReason !== null;

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
    () => (disabled ? null : findMentionQuery(composer.text, caret)),
    [composer.text, caret, disabled],
  );
  const hereChannel = target?.kind === "channel" ? target.channelId : ownChannel;
  const candidates = useMemo(
    () =>
      mention === null
        ? []
        : rankMentionCandidates(users.values(), mention.query, hereChannel, selfSession),
    [mention, users, hereChannel, selfSession],
  );
  const isDismissed =
    mention !== null &&
    dismissed !== null &&
    mention.start === dismissed.start &&
    mention.query.startsWith(dismissed.query);
  const pickerOpen = mention !== null && !isDismissed && candidates.length > 0;
  const activeIndex = Math.min(active, Math.max(candidates.length - 1, 0));
  const avatarOf = useAvatars(pickerOpen ? candidates : []);

  // Counts nested dragenter/dragleave pairs so the highlight does not flicker over child elements.
  const dragDepth = useRef(0);
  const [dragging, setDragging] = useState(false);
  // Native file drags (desktop app) over the composer; they bypass the HTML5 drag events.
  const [nativeOver, setNativeOver] = useState(false);
  const nativeDrop = useRef({ disabled, addDroppedPaths: composer.addDroppedPaths });
  useEffect(() => {
    nativeDrop.current = { disabled, addDroppedPaths: composer.addDroppedPaths };
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

  function pick(user: User) {
    const element = input.current;
    if (mention === null || element === null) return;
    const edit = insertMention(element.value, mention.start, element.selectionStart, user.name);
    pendingSelection.current = {
      text: edit.text,
      selectionStart: edit.caret,
      selectionEnd: edit.caret,
    };
    setDismissed({ start: mention.start, query: `${user.name} ` });
    setCaret(edit.caret);
    composer.setText(edit.text);
  }

  /** Handles the keys of the open mention list; true when the key was used. */
  function onPickerKey(event: KeyboardEvent<HTMLTextAreaElement>): boolean {
    if (!pickerOpen || event.nativeEvent.isComposing) return false;
    const count = candidates.length;
    switch (event.key) {
      case "ArrowDown":
        setActive((activeIndex + 1) % count);
        break;
      case "ArrowUp":
        setActive((activeIndex + count - 1) % count);
        break;
      case "Enter":
      case "Tab": {
        const user = candidates[activeIndex];
        if (user === undefined || event.shiftKey || event.ctrlKey || event.altKey) return false;
        pick(user);
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
      composer.submit();
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
      if (imageListed || hasHiddenFiles(data) || !hasText(data)) {
        event.preventDefault();
        void composer.addFromClipboard(imageListed);
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
          composer.submit();
        }}
      >
        {blockedReason ? <p className="chat-composer__blocked">{blockedReason}</p> : null}
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
                <Icon d={CANCEL_ICON} />
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
        <AttachmentTray items={composer.attachments} onRemove={composer.removeAttachment} />
        {pickerOpen ? (
          <MentionPicker
            id={pickerId}
            candidates={candidates}
            active={activeIndex}
            avatarOf={avatarOf}
            onPick={pick}
          />
        ) : null}
        <p className="visually-hidden" role="status">
          {pickerOpen ? t("mention.found", { count: candidates.length }) : ""}
        </p>
        <FormatToolbar onFormat={format} disabled={disabled} />
        <div className="chat-composer__row">
          <ChatActionRow />
          <button
            type="button"
            className="chat-composer__icon"
            aria-label={t("composer.addImage")}
            data-tooltip={t("composer.addImage")}
            disabled={disabled}
            onClick={() => fileInput.current?.click()}
          >
            <Icon d={IMAGE_ICON} />
          </button>
          <GifPicker onSend={composer.sendLink} disabled={disabled} linkPreviews={linkPreviews} />
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
          <textarea
            ref={input}
            role={pickerOpen ? "combobox" : undefined}
            aria-expanded={pickerOpen ? true : undefined}
            aria-controls={pickerOpen ? pickerId : undefined}
            aria-activedescendant={pickerOpen ? optionId(pickerId, activeIndex) : undefined}
            aria-autocomplete={pickerOpen ? "list" : undefined}
            className="chat-composer__input"
            rows={1}
            aria-label={label}
            aria-describedby={composer.error ? `${hintId} ${errorId}` : hintId}
            placeholder={label}
            value={composer.text}
            disabled={disabled}
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
          <button
            type="submit"
            className="chat-composer__icon"
            aria-label={t("composer.send")}
            data-tooltip={t("composer.send")}
            disabled={disabled}
          >
            <Icon d={SEND_ICON} />
          </button>
        </div>
        <p id={hintId} className="visually-hidden">
          {t("composer.hint")}
        </p>
      </form>
    </>
  );
}
