/**
 * Markdown formatting for the message input (FR-1.54): wraps the selection in Markdown markers,
 * or inserts empty markers with the caret between them, and maps keyboard shortcuts to formats.
 */
import { SAFE_LINK } from "./sanitize";

export type Format = "bold" | "italic" | "strike" | "code" | "codeBlock" | "link";

export const FORMATS: readonly Format[] = ["bold", "italic", "strike", "code", "codeBlock", "link"];

/** Shortcut per format in `aria-keyshortcuts` syntax; shown in tooltips as well. */
export const SHORTCUTS: Record<Format, string> = {
  bold: "Control+B",
  italic: "Control+I",
  strike: "Control+Shift+X",
  code: "Control+E",
  codeBlock: "Control+Shift+E",
  link: "Control+K",
};

export interface Edit {
  text: string;
  selectionStart: number;
  selectionEnd: number;
}

function wrap(edit: Edit, before: string, after: string): Edit {
  const { text, selectionStart: start, selectionEnd: end } = edit;
  return {
    text: text.slice(0, start) + before + text.slice(start, end) + after + text.slice(end),
    selectionStart: start + before.length,
    selectionEnd: end + before.length,
  };
}

/** Applies a format to the selected part of `edit.text`; the returned selection is the new caret. */
export function applyFormat(edit: Edit, format: Format): Edit {
  const { text, selectionStart: start, selectionEnd: end } = edit;
  switch (format) {
    case "bold":
      return wrap(edit, "**", "**");
    case "italic":
      return wrap(edit, "*", "*");
    case "strike":
      return wrap(edit, "~~", "~~");
    case "code":
      return wrap(edit, "`", "`");
    case "codeBlock": {
      // A fence only counts at the start of a line.
      const before = start === 0 || text[start - 1] === "\n" ? "```\n" : "\n```\n";
      const after = end === text.length || text[end] === "\n" ? "\n```" : "\n```\n";
      return wrap(edit, before, after);
    }
    case "link": {
      const selected = text.slice(start, end);
      if (SAFE_LINK.test(selected.trim())) {
        // A selected address becomes the target; the caret goes where the link text belongs.
        const next = wrap(edit, "[](", ")");
        return { ...next, selectionStart: start + 1, selectionEnd: start + 1 };
      }
      if (selected === "") return wrap(edit, "[", "]()");
      const caret = end + 3; // after "[" + selection + "]("
      return { ...wrap(edit, "[", "]()"), selectionStart: caret, selectionEnd: caret };
    }
  }
}

interface ShortcutKey {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}

/** The format a key press asks for (Ctrl, or Cmd on macOS), or null for any other key. */
export function formatForKey(event: ShortcutKey): Format | null {
  if (!(event.ctrlKey || event.metaKey) || event.altKey) return null;
  const key = event.key.toLowerCase();
  if (event.shiftKey) {
    if (key === "x") return "strike";
    if (key === "e") return "codeBlock";
    return null;
  }
  if (key === "b") return "bold";
  if (key === "i") return "italic";
  if (key === "e") return "code";
  if (key === "k") return "link";
  return null;
}
