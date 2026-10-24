/**
 * Turns what the user typed (Markdown shortcuts) into HTML the official Mumble client renders:
 * plain inline tags, `<br>` line breaks and no wrapping paragraph for single-paragraph
 * messages. Raw HTML typed by the user is shown as text, never interpreted.
 */
import { Marked } from "marked";
import { sanitizeHtml } from "./sanitize";

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
  renderer: {
    html({ text }) {
      return escapeHtml(text);
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
