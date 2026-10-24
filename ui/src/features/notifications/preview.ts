/** Longest preview in a notification, in characters. */
export const MAX_PREVIEW = 120;

const BLOCKS = "br, p, div, li, tr, h1, h2, h3, h4, h5, h6, blockquote, pre";

/**
 * Short plain-text preview of a message body (sanitized HTML from the engine). Parsing into an
 * inert document never runs scripts or loads images; only the text survives, so no markup or
 * image ever reaches a notification. Empty when the message has no text (e.g. only an image).
 */
export function plainPreview(html: string): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  for (const block of doc.body.querySelectorAll(BLOCKS)) block.after(" ");
  const text = doc.body.textContent.replace(/\s+/g, " ").trim();
  const chars = Array.from(text);
  if (chars.length <= MAX_PREVIEW) return text;
  return `${chars
    .slice(0, MAX_PREVIEW - 1)
    .join("")
    .trimEnd()}…`;
}

/** True when the body shows an image (used to say "sent an image" instead of nothing). */
export function hasImage(html: string): boolean {
  return new DOMParser().parseFromString(html, "text/html").querySelector("img") !== null;
}
