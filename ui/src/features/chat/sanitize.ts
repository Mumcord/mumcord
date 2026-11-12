/**
 * Second sanitizing layer for chat HTML (the engine already cleans it with ammonia). Server
 * content is untrusted: only basic formatting, links and inline `data:` images survive; nothing
 * may run scripts or load anything from the network.
 */
import DOMPurify from "dompurify";
import { canonicalLink } from "./mumbleLink";
import { FULL_ATTRIBUTE, FULL_REFERENCE } from "./fullImage";
import { IMAGE_REF_ATTRIBUTE, IMAGE_REFERENCE, withPlaceholders } from "./historyImages";

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
  "font",
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

/** `mumble://` links keep their text but never a target, and never their credentials (issue #175). */
const MUMBLE_TEXT = /mumble:\/\/[^\s<>"']+/gi;

/** Inline images only; anything else would be an external load. */
const SAFE_IMAGE = /^data:image\/(?:png|jpeg|gif|webp);base64,[a-z0-9+/=\s]+$/i;

/**
 * A text colour as Mumble writes it (`#rgb`, `#rrggbb` or `rgb(r, g, b)`), returned as
 * `#rrggbb`; anything else is `null`.
 */
export function normalizeColor(value: string): string | null {
  const text = value.trim().toLowerCase();
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/.exec(text);
  if (hex?.[1] !== undefined) {
    const digits = hex[1];
    return `#${digits.length === 3 ? digits.replace(/./g, (d) => d + d) : digits}`;
  }
  const rgb = /^rgb\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*\)$/.exec(text);
  if (!rgb) return null;
  const parts = rgb.slice(1).map(Number);
  if (parts.some((part) => part > 255)) return null;
  return `#${parts.map((part) => part.toString(16).padStart(2, "0")).join("")}`;
}

/** The colour of a `style` attribute that sets nothing but a text colour (issue #666). */
export function colorOfStyle(style: string): string | null {
  const match = /^\s*color\s*:\s*([^;]+?)\s*;?\s*$/i.exec(style);
  return match?.[1] === undefined ? null : normalizeColor(match[1]);
}

// A private instance keeps these hooks away from any other DOMPurify user in the app.
const purifier = DOMPurify(window);

// The full-image reference of message images (FR-1.60c) survives only in its exact format;
// data attributes are otherwise not allowed. The same goes for the card and code language markers.
purifier.addHook("uponSanitizeAttribute", (node, data) => {
  if (node.nodeName === "IMG" && data.attrName === FULL_ATTRIBUTE) {
    data.forceKeepAttr = FULL_REFERENCE.test(data.attrValue);
  } else if (node.nodeName === "IMG" && data.attrName === IMAGE_REF_ATTRIBUTE) {
    // A synced image that is still being fetched (FR-1.60f); becomes a placeholder below.
    data.forceKeepAttr = IMAGE_REFERENCE.test(data.attrValue);
  } else if (node.nodeName === "DIV" && data.attrName === CARD_ATTRIBUTE) {
    // Plain data; `cards.ts` parses the value strictly.
    data.forceKeepAttr = true;
  } else if (node.nodeName === "PRE" && data.attrName === CODE_LANGUAGE_ATTRIBUTE) {
    data.forceKeepAttr = CODE_LANGUAGE.test(data.attrValue);
  } else if (node.nodeName === "SPAN" && data.attrName === "style") {
    // Text colour, the way Mumble writes it; any other style could hide or overlay content.
    data.forceKeepAttr = colorOfStyle(data.attrValue) !== null;
  } else if (node.nodeName === "FONT" && data.attrName === "color") {
    data.forceKeepAttr = normalizeColor(data.attrValue) !== null;
  }
});

purifier.addHook("afterSanitizeAttributes", (node) => {
  if (node.nodeName === "A") {
    const href = node.getAttribute("href");
    if (href !== null && !SAFE_LINK.test(href.trim())) node.removeAttribute("href");
    node.setAttribute("rel", "noopener noreferrer");
  } else if (node.nodeName === "IMG") {
    const src = node.getAttribute("src");
    const waiting = src === null && node.hasAttribute(IMAGE_REF_ATTRIBUTE);
    if (!waiting && (src === null || !SAFE_IMAGE.test(src.trim()))) node.remove();
  } else if (node.nodeName === "SPAN" || node.nodeName === "FONT") {
    // Kept colours are written back in one canonical form, so cleaning twice changes nothing.
    const name = node.nodeName === "SPAN" ? "style" : "color";
    const value = node.getAttribute(name);
    const color =
      value === null ? null : name === "style" ? colorOfStyle(value) : normalizeColor(value);
    if (color !== null) node.setAttribute(name, name === "style" ? `color:${color}` : color);
  }
});

/** Returns HTML that is safe to put into the DOM. */
export function sanitizeHtml(html: string): string {
  const clean = purifier.sanitize(html, {
    ALLOWED_TAGS,
    ALLOWED_ATTR,
    // `rel` is set by the hook above; users cannot supply it.
    ADD_ATTR: ["rel"],
    ALLOW_DATA_ATTR: false,
    ALLOWED_URI_REGEXP: /^(?:https?:|mailto:|data:image\/(?:png|jpeg|gif|webp);base64,|mumble:)/i,
  });
  // A `mumble://` link is never opened by itself, and its credentials never reach the screen
  // (issue #175). The card under the message has the buttons.
  return withPlaceholders(clean.replace(MUMBLE_TEXT, (match) => canonicalLink(match)));
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
