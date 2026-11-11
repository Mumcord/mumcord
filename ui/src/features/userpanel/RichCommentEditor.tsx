import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ClipboardEvent,
  type CSSProperties,
  type KeyboardEvent,
} from "react";
import { useTranslation } from "react-i18next";
import { imageTag, prepareImage, type ImageResult } from "../chat/images";
import { cleanCommentHtml, commentToSend } from "./commentHtml";
import {
  caretState,
  EMPTY_CARET,
  escapeHtml,
  measureHtml,
  removeColor,
  unwrap,
  type CaretState,
  type EditorLimits,
} from "./richEditorModel";
import { ColorPopover, LinkPopover } from "./RichEditorPopovers";
import { RichEditorStatus, type EditorNotice } from "./RichEditorStatus";
import { RichEditorToolbar, type Tool } from "./RichEditorToolbar";
import "./richEditor.css";

interface RichCommentEditorProps {
  labelId: string;
  /** Comment HTML to start from; applied again whenever `loadKey` changes. */
  initialHtml: string;
  loadKey: number;
  disabled: boolean;
  describedBy?: string | undefined;
  onChange: (html: string) => void;
  /** What the text is for; only the hint under it differs. */
  purpose: "comment" | "description";
  /** The server's limits; the meter and the image scaling follow them. */
  limits: EditorLimits;
}

// `execCommand` is deprecated but is the one editing API every webview we ship on supports and
// it writes the plain <b>/<i>/<ul>/<font color> markup Mumble clients expect.
const COMMAND: Record<Tool, string> = {
  bold: "bold",
  italic: "italic",
  underline: "underline",
  strike: "strikeThrough",
  bullets: "insertUnorderedList",
  numbers: "insertOrderedList",
};

const LETTER_TOOL: Partial<Record<string, Tool | "link">> = {
  b: "bold",
  i: "italic",
  u: "underline",
  k: "link",
};

/** Already fitting images of these types are embedded as they are; others become PNG or JPEG. */
const KEEP_IMAGE_TYPES = ["image/png", "image/jpeg"];

const IMAGE_ERROR = {
  notImage: "editor.imageNotImage",
  unreadable: "editor.imageUnreadable",
  tooLarge: "editor.imageTooLarge",
  sourceTooLarge: "editor.imageSourceTooLarge",
} as const;

function run(command: string, value?: string) {
  // eslint-disable-next-line @typescript-eslint/no-deprecated
  document.execCommand(command, false, value);
}

/** Formatting the webview will apply to the next typed text (e.g. Bold pressed at a caret). */
function pendingState(command: string): boolean {
  try {
    // eslint-disable-next-line @typescript-eslint/no-deprecated
    return document.queryCommandState(command);
  } catch {
    // Not every engine implements it (jsdom does not); the markup around the caret still counts.
    return false;
  }
}

type Popup =
  | { kind: "color"; style: CSSProperties }
  | { kind: "link"; style: CSSProperties; text: string; address: string; editing: boolean };

/**
 * Rich text editor for the own comment and channel descriptions (FR-2.51, issue #666): one box
 * with the toolbar on top, a Source (HTML) view and a meter for the server's length limit.
 */
export function RichCommentEditor({
  labelId,
  initialHtml,
  loadKey,
  disabled,
  describedBy,
  onChange,
  purpose,
  limits,
}: RichCommentEditorProps) {
  const { t } = useTranslation("userpanel");
  const root = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLDivElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const savedRange = useRef<Range | null>(null);
  // The link the open popover edits; `null` when it creates a new one.
  const linkTarget = useRef<HTMLAnchorElement | null>(null);
  const [loadedKey, setLoadedKey] = useState(loadKey);
  const [html, setHtml] = useState(() => cleanCommentHtml(initialHtml));
  // The HTML being edited in the Source view; `null` while the formatted view is shown.
  const [source, setSource] = useState<string | null>(null);
  const [caret, setCaret] = useState<CaretState>(EMPTY_CARET);
  const [popup, setPopup] = useState<Popup | null>(null);
  const [notice, setNotice] = useState<EditorNotice | null>(null);

  // Only a new load replaces what the user may already have typed.
  if (loadedKey !== loadKey) {
    setLoadedKey(loadKey);
    setHtml(cleanCommentHtml(initialHtml));
    setSource(null);
    setNotice(null);
  }
  useEffect(() => {
    if (field.current) field.current.innerHTML = cleanCommentHtml(initialHtml);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loadKey]);

  const measure = useMemo(
    () => measureHtml(commentToSend(source ?? html), limits),
    [source, html, limits],
  );

  const refreshCaret = useCallback(() => {
    const element = field.current;
    const node = window.getSelection()?.anchorNode ?? null;
    if (!element || node === null || !element.contains(node)) return;
    const state = caretState(node, element);
    const formats = new Set(state.formats);
    // Inside a link the webview reports its underline as "underline"; the markup decides there.
    const pending = state.link === null ? (["bold", "italic", "underline", "strike"] as const) : [];
    for (const tool of pending) {
      if (pendingState(COMMAND[tool])) formats.add(tool);
    }
    setCaret({ ...state, formats });
  }, []);

  useEffect(() => {
    document.addEventListener("selectionchange", refreshCaret);
    return () => {
      document.removeEventListener("selectionchange", refreshCaret);
    };
  }, [refreshCaret]);

  // A click anywhere else closes an open palette or link popover.
  const popupOpen = popup !== null;
  useEffect(() => {
    if (!popupOpen) return;
    const down = (event: MouseEvent) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      if (target.closest(".rte-pop") || target.closest("[data-popup-tool]")) return;
      setPopup(null);
    };
    document.addEventListener("mousedown", down);
    return () => {
      document.removeEventListener("mousedown", down);
    };
  }, [popupOpen]);

  const emit = (value: string) => {
    setHtml(value);
    onChange(value);
  };
  const changed = () => {
    emit(field.current?.innerHTML ?? "");
  };

  const saveRange = () => {
    const selection = window.getSelection();
    const range = selection && selection.rangeCount > 0 ? selection.getRangeAt(0) : null;
    savedRange.current =
      range && field.current?.contains(range.commonAncestorContainer) ? range.cloneRange() : null;
  };

  /** Puts the focus and the saved selection (or the end of the text) back into the field. */
  const restoreRange = (): Range | null => {
    const element = field.current;
    if (!element) return null;
    element.focus();
    let range = savedRange.current;
    if (range === null) {
      range = document.createRange();
      range.selectNodeContents(element);
      range.collapse(false);
    }
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
    return range;
  };

  const format = (tool: Tool) => {
    field.current?.focus();
    run(COMMAND[tool]);
    changed();
    refreshCaret();
  };

  const popupStyle = (button: HTMLElement | null): CSSProperties =>
    button ? { left: button.offsetLeft, top: button.offsetTop + button.offsetHeight + 6 } : {};

  const openColor = (button: HTMLElement | null) => {
    if (popup?.kind === "color") {
      setPopup(null);
      return;
    }
    saveRange();
    setPopup({ kind: "color", style: popupStyle(button) });
  };

  const openLink = (button: HTMLElement | null) => {
    if (popup?.kind === "link") {
      setPopup(null);
      return;
    }
    saveRange();
    const range = savedRange.current;
    const element = field.current;
    const link = range && element ? caretState(range.startContainer, element).link : null;
    linkTarget.current = link;
    setPopup({
      kind: "link",
      style: popupStyle(button),
      text: link ? link.textContent : (range?.toString() ?? ""),
      address: link?.getAttribute("href") ?? "",
      editing: link !== null,
    });
  };

  const closePopup = () => {
    setPopup(null);
    restoreRange();
  };

  const pickColor = (color: string | null) => {
    const range = restoreRange();
    setPopup(null);
    if (color === null) {
      if (range && field.current) removeColor(range, field.current);
    } else {
      // Plain `<font color>`, which every Mumble client renders.
      run("styleWithCSS", "false");
      run("foreColor", color);
    }
    changed();
    refreshCaret();
  };

  const applyLink = (text: string, href: string) => {
    const target = linkTarget.current;
    setPopup(null);
    if (target && field.current?.contains(target)) {
      target.setAttribute("href", href);
      if (text.trim() !== "" && text !== target.textContent) target.textContent = text;
      restoreRange();
    } else {
      const range = restoreRange();
      const label = text.trim() === "" ? href : text;
      if (range && !range.collapsed && label === range.toString()) run("createLink", href);
      else run("insertHTML", `<a href="${escapeHtml(href)}">${escapeHtml(label)}</a>`);
    }
    changed();
    refreshCaret();
  };

  const removeLink = () => {
    const target = linkTarget.current;
    setPopup(null);
    if (target) unwrap(target);
    restoreRange();
    changed();
    refreshCaret();
  };

  const insertImage = async (file: File) => {
    const before = commentToSend(field.current?.innerHTML ?? "");
    // The whole text, not just the image, has to fit the server's limit.
    const budget = limits.image - before.length - imageTag("").length;
    setNotice({ kind: "info", text: t("editor.imageBusy") });
    const result: ImageResult =
      budget > 0
        ? await prepareImage(file, budget, KEEP_IMAGE_TYPES)
        : { ok: false, reason: "tooLarge" };
    if (!result.ok) {
      setNotice({ kind: "error", text: t(IMAGE_ERROR[result.reason]) });
      return;
    }
    restoreRange();
    run("insertHTML", imageTag(result.dataUrl));
    changed();
    setNotice(
      result.scaled
        ? {
            kind: "info",
            text: t("editor.imageScaled", { width: result.width, height: result.height }),
          }
        : null,
    );
  };

  const paste = (event: ClipboardEvent) => {
    const image = Array.from(event.clipboardData.files).find((f) => f.type.startsWith("image/"));
    if (image) {
      event.preventDefault();
      saveRange();
      void insertImage(image);
      return;
    }
    const pasted = event.clipboardData.getData("text/html");
    const text = event.clipboardData.getData("text/plain");
    // WebKitGTK may hide text offered only as `text/plain;charset=utf-8`; the webview inserts it.
    if (!pasted && !text) return;
    event.preventDefault();
    if (pasted) run("insertHTML", cleanCommentHtml(pasted));
    else run("insertText", text);
    changed();
  };

  const keyDown = (event: KeyboardEvent) => {
    if (!(event.ctrlKey || event.metaKey) || event.altKey) return;
    let tool: Tool | "link" | null = null;
    if (event.shiftKey) {
      if (event.key.toLowerCase() === "x") tool = "strike";
      else if (event.code === "Digit8") tool = "bullets";
      else if (event.code === "Digit7") tool = "numbers";
    } else {
      tool = LETTER_TOOL[event.key.toLowerCase()] ?? null;
    }
    if (tool === null) return;
    event.preventDefault();
    if (tool === "link") {
      openLink(root.current?.querySelector<HTMLElement>('[data-tool="link"]') ?? null);
    } else format(tool);
  };

  const toggleSource = () => {
    setPopup(null);
    if (source === null) {
      setSource(cleanCommentHtml(field.current?.innerHTML ?? ""));
      return;
    }
    const clean = cleanCommentHtml(source);
    if (field.current) field.current.innerHTML = clean;
    setSource(null);
    emit(clean);
  };

  const inSource = source !== null;
  const hint = inSource
    ? t("editor.hintSource")
    : purpose === "comment"
      ? t("editor.hintComment")
      : t("editor.hintDescription");

  return (
    <div className="rte" ref={root} data-level={measure.level}>
      <RichEditorToolbar
        caret={caret}
        popup={popup?.kind ?? null}
        inSource={inSource}
        disabled={disabled}
        onFormat={format}
        onColor={openColor}
        onLink={openLink}
        onImage={() => {
          saveRange();
          fileInput.current?.click();
        }}
        onSource={toggleSource}
      />
      {popup?.kind === "color" && (
        <ColorPopover
          current={caret.color}
          style={popup.style}
          onPick={pickColor}
          onClose={closePopup}
        />
      )}
      {popup?.kind === "link" && (
        <LinkPopover
          text={popup.text}
          address={popup.address}
          editing={popup.editing}
          style={popup.style}
          onApply={applyLink}
          onRemove={removeLink}
          onClose={closePopup}
        />
      )}
      <input
        ref={fileInput}
        type="file"
        accept="image/png,image/jpeg,image/gif,image/webp"
        hidden
        onChange={(event) => {
          const file = event.currentTarget.files?.[0];
          event.currentTarget.value = "";
          if (file) void insertImage(file);
        }}
      />
      <div
        ref={field}
        className="rte__field"
        role="textbox"
        aria-multiline="true"
        aria-labelledby={labelId}
        aria-describedby={describedBy}
        aria-disabled={disabled}
        hidden={inSource}
        tabIndex={disabled ? -1 : 0}
        contentEditable={!disabled}
        suppressContentEditableWarning
        onInput={() => {
          setNotice(null);
          changed();
        }}
        onPaste={paste}
        onKeyDown={keyDown}
        onKeyUp={refreshCaret}
        onMouseUp={refreshCaret}
      />
      {inSource && (
        <textarea
          className="rte__source"
          aria-labelledby={labelId}
          aria-describedby={describedBy}
          spellCheck={false}
          disabled={disabled}
          value={source}
          onChange={(e) => {
            setSource(e.target.value);
            onChange(e.target.value);
          }}
        />
      )}
      <RichEditorStatus hint={hint} notice={notice} measure={measure} />
    </div>
  );
}
