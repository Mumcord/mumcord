import { sanitizeHtml } from "../chat/sanitize";

/**
 * Cleans comment HTML down to the subset Mumble clients render. Used for what the server sends
 * us (untrusted), for pasted content and for what we send back, so formatting written by other
 * clients survives an edit but nothing active ever reaches the editor.
 */
export function cleanCommentHtml(html: string): string {
  return sanitizeHtml(html);
}

/** Elements without content; written self-closed so the markup is well-formed XML. */
const VOID_TAGS = new Set(["area", "br", "col", "hr", "img", "wbr"]);

function escapeXml(text: string): string {
  return text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
}

function toXml(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) return escapeXml(node.textContent ?? "");
  if (!(node instanceof Element)) return "";
  const name = node.nodeName.toLowerCase();
  const attributes = Array.from(node.attributes)
    .map((a) => ` ${a.name}="${escapeXml(a.value).replaceAll('"', "&quot;")}"`)
    .join("");
  if (VOID_TAGS.has(name)) return `<${name}${attributes} />`;
  return `<${name}${attributes}>${Array.from(node.childNodes).map(toXml).join("")}</${name}>`;
}

/**
 * The HTML to send: sanitized, and empty when nothing would be visible.
 *
 * Written as well-formed XML (`<img … />`, `<br />`, no `&nbsp;`): above its text limit a
 * Mumble server parses the text as XML to leave image data out of the count, and refuses
 * with "too long" whatever does not parse (issue #712).
 */
export function commentToSend(html: string): string {
  const doc = new DOMParser().parseFromString(cleanCommentHtml(html), "text/html");
  // Nothing visible: no text and no image.
  if (doc.body.textContent.trim() === "" && doc.body.querySelector("img") === null) return "";
  return Array.from(doc.body.childNodes).map(toXml).join("");
}

/** Only links an official client may open are accepted from the link field. */
export function normalizeLink(input: string): string | null {
  const value = input.trim();
  if (value === "") return null;
  if (/^(?:https?:|mailto:)/i.test(value)) return value;
  return /^[a-z][a-z0-9+.-]*:/i.test(value) ? null : `https://${value}`;
}
