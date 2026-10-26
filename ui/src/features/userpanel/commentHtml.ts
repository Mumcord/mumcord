import { sanitizeHtml } from "../chat/sanitize";

/**
 * Cleans comment HTML down to the subset Mumble clients render. Used for what the server sends
 * us (untrusted), for pasted content and for what we send back, so formatting written by other
 * clients survives an edit but nothing active ever reaches the editor.
 */
export function cleanCommentHtml(html: string): string {
  return sanitizeHtml(html);
}

/** True when the comment shows nothing: no text and no image. */
export function isBlankComment(html: string): boolean {
  const doc = new DOMParser().parseFromString(html, "text/html");
  return doc.body.textContent.trim() === "" && doc.body.querySelector("img") === null;
}

/** The HTML to send: sanitized, and empty when nothing would be visible. */
export function commentToSend(html: string): string {
  const clean = cleanCommentHtml(html);
  return isBlankComment(clean) ? "" : clean;
}

/** Only links an official client may open are accepted from the link field. */
export function normalizeLink(input: string): string | null {
  const value = input.trim();
  if (value === "") return null;
  if (/^(?:https?:|mailto:)/i.test(value)) return value;
  return /^[a-z][a-z0-9+.-]*:/i.test(value) ? null : `https://${value}`;
}
