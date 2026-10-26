import { useEffect, useRef, useState, type ClipboardEvent, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { cleanCommentHtml, normalizeLink } from "./commentHtml";

interface RichCommentEditorProps {
  labelId: string;
  /** Comment HTML to start from; applied again whenever `loadKey` changes. */
  initialHtml: string;
  loadKey: number;
  disabled: boolean;
  describedBy?: string | undefined;
  onChange: (html: string) => void;
}

type Command = "bold" | "italic" | "insertUnorderedList" | "insertOrderedList";

// `execCommand` is deprecated but is the one editing API every webview we ship on supports and
// it writes the plain <b>/<i>/<ul> markup Mumble clients expect.
function run(command: string, value?: string) {
  // eslint-disable-next-line @typescript-eslint/no-deprecated
  document.execCommand(command, false, value);
}

export function RichCommentEditor({
  labelId,
  initialHtml,
  loadKey,
  disabled,
  describedBy,
  onChange,
}: RichCommentEditorProps) {
  const { t } = useTranslation("userpanel");
  const editor = useRef<HTMLDivElement>(null);
  const savedRange = useRef<Range | null>(null);
  const linkInput = useRef<HTMLInputElement>(null);
  const [linkUrl, setLinkUrl] = useState<string | null>(null);

  const linkOpen = linkUrl !== null;
  useEffect(() => {
    if (linkOpen) linkInput.current?.focus();
  }, [linkOpen]);

  useEffect(() => {
    if (editor.current) editor.current.innerHTML = cleanCommentHtml(initialHtml);
    // Only a new load replaces what the user may already have typed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loadKey]);

  const changed = () => {
    onChange(editor.current?.innerHTML ?? "");
  };

  const format = (command: Command) => {
    editor.current?.focus();
    run(command);
    changed();
  };

  const startLink = () => {
    const selection = window.getSelection();
    savedRange.current =
      selection && selection.rangeCount > 0 ? selection.getRangeAt(0).cloneRange() : null;
    setLinkUrl("");
  };

  const applyLink = () => {
    const href = normalizeLink(linkUrl ?? "");
    setLinkUrl(null);
    editor.current?.focus();
    const selection = window.getSelection();
    if (savedRange.current && selection) {
      selection.removeAllRanges();
      selection.addRange(savedRange.current);
    }
    if (href === null) return;
    if (selection?.isCollapsed) {
      // Nothing selected: the address itself becomes the link text.
      const anchor = document.createElement("a");
      anchor.href = href;
      anchor.textContent = href;
      run("insertHTML", anchor.outerHTML);
    } else run("createLink", href);
    changed();
  };

  const paste = (event: ClipboardEvent) => {
    event.preventDefault();
    const html = event.clipboardData.getData("text/html");
    if (html) run("insertHTML", cleanCommentHtml(html));
    else run("insertText", event.clipboardData.getData("text/plain"));
    changed();
  };

  const keyDown = (event: KeyboardEvent) => {
    if (!(event.ctrlKey || event.metaKey)) return;
    const key = event.key.toLowerCase();
    if (key === "b" || key === "i") {
      event.preventDefault();
      format(key === "b" ? "bold" : "italic");
    } else if (key === "k") {
      event.preventDefault();
      startLink();
    }
  };

  const tool = (label: string, command: Command, text: string) => (
    <button
      type="button"
      className="button rich-editor__tool"
      aria-label={label}
      data-tooltip={label}
      disabled={disabled}
      onMouseDown={(e) => {
        e.preventDefault();
      }}
      onClick={() => {
        format(command);
      }}
    >
      {text}
    </button>
  );

  return (
    <div className="rich-editor">
      <div role="toolbar" aria-label={t("profile.format")} className="rich-editor__toolbar">
        {tool(t("profile.bold"), "bold", "B")}
        {tool(t("profile.italic"), "italic", "I")}
        <button
          type="button"
          className="button rich-editor__tool"
          aria-label={t("profile.link")}
          data-tooltip={t("profile.link")}
          disabled={disabled}
          onMouseDown={(e) => {
            e.preventDefault();
          }}
          onClick={startLink}
        >
          🔗
        </button>
        {tool(t("profile.bulletList"), "insertUnorderedList", "•")}
        {tool(t("profile.numberedList"), "insertOrderedList", "1.")}
      </div>
      {linkUrl !== null ? (
        <div className="rich-editor__link">
          <input
            type="text"
            ref={linkInput}
            aria-label={t("profile.linkAddress")}
            placeholder="https://"
            value={linkUrl}
            onChange={(e) => {
              setLinkUrl(e.target.value);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                applyLink();
              } else if (e.key === "Escape") {
                e.stopPropagation();
                setLinkUrl(null);
              }
            }}
          />
          <button type="button" className="button" onClick={applyLink}>
            {t("profile.linkApply")}
          </button>
        </div>
      ) : null}
      <div
        ref={editor}
        className="rich-editor__field"
        role="textbox"
        aria-multiline="true"
        aria-labelledby={labelId}
        aria-describedby={describedBy}
        aria-disabled={disabled}
        tabIndex={disabled ? -1 : 0}
        contentEditable={!disabled}
        suppressContentEditableWarning
        onInput={changed}
        onPaste={paste}
        onKeyDown={keyDown}
      />
    </div>
  );
}
