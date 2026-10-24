/**
 * Message input: Markdown with a formatting toolbar and shortcuts, Enter to send (Shift+Enter for a new line), images by
 * button, paste or drop, and inline errors such as a denied permission or a too-large image.
 */
import {
  useId,
  useLayoutEffect,
  useRef,
  type ClipboardEvent,
  type DragEvent,
  type KeyboardEvent,
} from "react";
import { useTranslation } from "react-i18next";
import type { SendTarget } from "./conversationInfo";
import { FormatToolbar } from "./FormatToolbar";
import { applyFormat, formatForKey, type Edit, type Format } from "./formatting";
import type { MessageLimits } from "./images";
import { useComposer } from "./useComposer";

export interface ComposerProps {
  target: SendTarget | null;
  limits: MessageLimits;
  /** Accessible name and placeholder, e.g. "Message #Lobby". */
  label: string;
  /** Why writing is impossible right now (no permission, person offline); disables the input. */
  blockedReason: string | null;
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

function firstImage(files: FileList | null | undefined): File | undefined {
  return Array.from(files ?? []).find((file) => file.type.startsWith("image/"));
}

export function Composer({ target, limits, label, blockedReason }: ComposerProps) {
  const { t } = useTranslation("chat");
  const composer = useComposer(target, limits);
  const fileInput = useRef<HTMLInputElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  // Selection to restore once a formatting edit has been rendered into the textarea.
  const pendingSelection = useRef<Edit | null>(null);
  const hintId = useId();
  const errorId = useId();
  const disabled = blockedReason !== null;

  function attach(file: File | undefined) {
    if (file) void composer.addImage(file);
  }

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

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    const shortcut = formatForKey(event);
    if (shortcut !== null) {
      // Also tells global shortcuts (Ctrl+K channel search) that the key was handled here.
      event.preventDefault();
      format(shortcut);
      return;
    }
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      void composer.submit();
    }
  }

  function onPaste(event: ClipboardEvent<HTMLTextAreaElement>) {
    const file = firstImage(event.clipboardData.files);
    if (!file) return;
    event.preventDefault();
    attach(file);
  }

  function onDrop(event: DragEvent<HTMLTextAreaElement>) {
    const file = firstImage(event.dataTransfer.files);
    if (!file) return;
    event.preventDefault();
    attach(file);
  }

  return (
    <form
      className="chat-composer"
      onSubmit={(event) => {
        event.preventDefault();
        void composer.submit();
      }}
    >
      {blockedReason ? <p className="chat-composer__blocked">{blockedReason}</p> : null}
      {composer.error ? (
        <p id={errorId} className="chat-composer__error" role="alert">
          {composer.error}
        </p>
      ) : null}
      {composer.image ? (
        <div className="chat-composer__preview">
          <img src={composer.image} alt={t("composer.imagePreview")} />
          <button type="button" className="chat-composer__icon" onClick={composer.removeImage}>
            {t("composer.removeImage")}
          </button>
        </div>
      ) : null}
      <FormatToolbar onFormat={format} disabled={disabled} />
      <div className="chat-composer__row">
        <button
          type="button"
          className="chat-composer__icon"
          aria-label={t("composer.addImage")}
          title={t("composer.addImage")}
          disabled={disabled || composer.busy}
          onClick={() => fileInput.current?.click()}
        >
          <Icon d={IMAGE_ICON} />
        </button>
        <input
          ref={fileInput}
          type="file"
          accept="image/png,image/jpeg,image/gif,image/webp"
          hidden
          onChange={(event) => {
            attach(firstImage(event.currentTarget.files));
            event.currentTarget.value = "";
          }}
        />
        <textarea
          ref={input}
          className="chat-composer__input"
          rows={1}
          aria-label={label}
          aria-describedby={composer.error ? `${hintId} ${errorId}` : hintId}
          placeholder={label}
          value={composer.text}
          disabled={disabled}
          onChange={(event) => {
            composer.setText(event.target.value);
          }}
          onKeyDown={onKeyDown}
          onPaste={onPaste}
          onDrop={onDrop}
        />
        <button
          type="submit"
          className="chat-composer__icon"
          aria-label={t("composer.send")}
          title={t("composer.send")}
          disabled={disabled || composer.busy}
        >
          <Icon d={SEND_ICON} />
        </button>
      </div>
      <p id={hintId} className="visually-hidden">
        {t("composer.hint")}
      </p>
    </form>
  );
}
