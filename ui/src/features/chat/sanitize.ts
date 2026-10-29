/**
 * Second sanitizing layer for chat HTML (the engine already cleans it with ammonia). Server
 * content is untrusted: only basic formatting, links and inline `data:` images survive; nothing
 * may run scripts or load anything from the network.
 */
import DOMPurify from "dompurify";
import { FULL_ATTRIBUTE, FULL_REFERENCE } from "./fullImage";

/** Marks a message card on the wire (see `cards.ts`); kept on `<div>` only. */
const CARD_ATTRIBUTE = "data-mumcord-card";

/**
 * The language of a ``` code block on the wire, kept on `<pre>` only (issue #112). The class
 * attribute does not survive Mumble, a data attribute does, like the card marker.
 */
export const CODE_LANGUAGE_ATTRIBUTE = "data-mumcord-lang";

/** A language name as typed after the fence: `sql`, `c++`, `objective-c`; nothing else. */
export const CODE_LANGUAGE = /^[a-z0-9][a-z0-9+#._-]{0,31}$/;

const ALLOWED_TAGS = [
  "a",
  "b",
  "blockquote",
  "br",
  "code",
  "del",
  "div",
  "em",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "hr",
  "i",
  "img",
  "li",
  "ol",
  "p",
  "pre",
  "s",
  "span",
  "strike",
  "strong",
  "sub",
  "sup",
  "table",
  "tbody",
  "td",
  "th",
  "thead",
  "tr",
  "u",
  "ul",
];

const ALLOWED_ATTR = ["href", "src", "alt", "title"];

/** Link targets a click may hand to the system browser. */
export const SAFE_LINK = /^(?:https?:|mailto:)/i;

/** Inline images only; anything else would be an external load. */
const SAFE_IMAGE = /^data:image\/(?:png|jpeg|gif|webp);base64,[a-z0-9+/=\s]+$/i;

// A private instance keeps these hooks away from any other DOMPurify user in the app.
const purifier = DOMPurify(window);

// The full-image reference of message images (FR-1.60c) survives only in its exact format;
// data attributes are otherwise not allowed. The same goes for the card and code language markers.
purifier.addHook("uponSanitizeAttribute", (node, data) => {
  if (node.nodeName === "IMG" && data.attrName === FULL_ATTRIBUTE) {
    data.forceKeepAttr = FULL_REFERENCE.test(data.attrValue);
  } else if (node.nodeName === "DIV" && data.attrName === CARD_ATTRIBUTE) {
    // Plain data; `cards.ts` parses the value strictly.
    data.forceKeepAttr = true;
  } else if (node.nodeName === "PRE" && data.attrName === CODE_LANGUAGE_ATTRIBUTE) {
    data.forceKeepAttr = CODE_LANGUAGE.test(data.attrValue);
  }
});

purifier.addHook("afterSanitizeAttributes", (node) => {
  if (node.nodeName === "A") {
    const href = node.getAttribute("href");
    if (href !== null && !SAFE_LINK.test(href.trim())) node.removeAttribute("href");
    node.setAttribute("rel", "noopener noreferrer");
  } else if (node.nodeName === "IMG") {
    const src = node.getAttribute("src");
    if (src === null || !SAFE_IMAGE.test(src.trim())) node.remove();
  }
});

/** Returns HTML that is safe to put into the DOM. */
export function sanitizeHtml(html: string): string {
  return purifier.sanitize(html, {
    ALLOWED_TAGS,
    ALLOWED_ATTR,
    // `rel` is set by the hook above; users cannot supply it.
    ADD_ATTR: ["rel"],
    ALLOW_DATA_ATTR: false,
    ALLOWED_URI_REGEXP: /^(?:https?:|mailto:|data:image\/(?:png|jpeg|gif|webp);base64,)/i,
  });
}

/** Visible text of a message, e.g. for screen-reader announcements. */
export function htmlToText(html: string): string {
  const doc = new DOMParser().parseFromString(sanitizeHtml(html), "text/html");
  return doc.body.textContent.replace(/\s+/g, " ").trim();
}

/** True when the message contains an inline image that survives sanitizing. */
export function hasImage(html: string): boolean {
  return sanitizeHtml(html).includes("<img");
}
