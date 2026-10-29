/**
 * Turns what the user typed (Markdown shortcuts) into HTML the official Mumble client renders:
 * plain inline tags, `<br>` line breaks and no wrapping paragraph for single-paragraph
 * messages. Raw HTML typed by the user is shown as text, never interpreted.
 */
import { Marked } from "marked";
import { CODE_LANGUAGE, CODE_LANGUAGE_ATTRIBUTE, sanitizeHtml } from "./sanitize";

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

const marked = new Marked({
  gfm: true,
  breaks: true,
  tokenizer: {
    // Only `> ` with a space starts a quote (FR-2.68); `>_>`, `>.<` or `>hello` stay text (FR-1.30).
    blockquote(src) {
      const match = /^(?:> [^\n]*(?:\n|$))+/.exec(src);
      if (match === null) return undefined;
      const raw = match[0];
      const text = raw
        .replace(/\n$/, "")
        .split("\n")
        .map((line) => line.slice(2))
        .join("\n");
      return { type: "blockquote", raw, text, tokens: this.lexer.blockTokens(text) };
    },
  },
  renderer: {
    html({ text }) {
      return escapeHtml(text);
    },
    // The fence language travels as a data attribute (issue #112); official clients ignore it.
    code({ text, lang }) {
      const language = (lang ?? "").trim().split(/\s/, 1)[0]?.toLowerCase() ?? "";
      const marker = CODE_LANGUAGE.test(language)
        ? ` ${CODE_LANGUAGE_ATTRIBUTE}="${escapeHtml(language)}"`
        : "";
      return `<pre${marker}><code>${escapeHtml(text.replace(/\n$/, ""))}\n</code></pre>\n`;
    },
  },
});

/** Markdown → sanitized, Mumble-compatible HTML. Empty input gives an empty string. */
export function markdownToHtml(source: string): string {
  const trimmed = source.trim();
  if (trimmed === "") return "";
  let html = marked.parse(trimmed, { async: false }).trim();
  const single = /^<p>([\s\S]*)<\/p>$/.exec(html);
  if (single?.[1] !== undefined && !single[1].includes("<p>")) html = single[1];
  // Newlines between block tags would show up as extra spaces in Qt's rich text.
  return sanitizeHtml(html.replace(/>\n</g, "><"));
}

/** For servers that do not allow HTML: the text as-is, safely escaped. */
export function plainTextToHtml(source: string): string {
  return escapeHtml(source.trim());
}
