/**
 * Logic of the rich text editor for comments and channel descriptions (issue #666): the server's
 * length limits, the formatting at the caret and the edits that `execCommand` cannot do cleanly.
 */
import type { ServerInfo } from "../../api";
import { colorOfStyle, normalizeColor } from "../chat/sanitize";
import { commentToSend } from "./commentHtml";

/** Size limits in characters, as the server checks comments and descriptions. */
export interface EditorLimits {
  /** Without images (`message_length`). */
  text: number;
  /** With images (`image_message_length`). */
  image: number;
}

/** Mumble's defaults, used while the server has not reported its own limits. */
export const DEFAULT_LIMITS: EditorLimits = { text: 5000, image: 131072 };

/** From this share of the limit on, the meter warns. */
const NEAR_LIMIT = 0.85;

export function editorLimits(info: ServerInfo | null): EditorLimits {
  return {
    text: info && info.maxMessageLength > 0 ? info.maxMessageLength : DEFAULT_LIMITS.text,
    image:
      info && info.maxImageMessageLength > 0 ? info.maxImageMessageLength : DEFAULT_LIMITS.image,
  };
}

export type LimitLevel = "ok" | "near" | "over";

export interface LengthMeasure {
  used: number;
  limit: number;
  /** The HTML contains an image, so the image limit applies. */
  images: boolean;
  level: LimitLevel;
}

/**
 * Measures cleaned HTML the way a Mumble server does: its length in UTF-16 units (Qt's
 * `QString::length`) against `message_length`, or `image_message_length` once it has an image.
 */
export function measureHtml(html: string, limits: EditorLimits): LengthMeasure {
  const images = /<img\b/i.test(html);
  const limit = images ? limits.image : limits.text;
  const used = html.length;
  const level = used > limit ? "over" : used > limit * NEAR_LIMIT ? "near" : "ok";
  return { used, limit, images, level };
}

/** True when the HTML, as it would be sent, is over the server's limit. */
export function isTooLong(html: string, limits: EditorLimits): boolean {
  return measureHtml(commentToSend(html), limits).level === "over";
}

export type Format = "bold" | "italic" | "underline" | "strike" | "bullets" | "numbers" | "link";

const FORMAT_OF_TAG: Record<string, Format> = {
  B: "bold",
  STRONG: "bold",
  I: "italic",
  EM: "italic",
  U: "underline",
  S: "strike",
  STRIKE: "strike",
  DEL: "strike",
  A: "link",
};

export interface CaretState {
  formats: ReadonlySet<Format>;
  /** `#rrggbb` of the nearest coloured element, or `null` for the default colour. */
  color: string | null;
  link: HTMLAnchorElement | null;
}

export const EMPTY_CARET: CaretState = { formats: new Set(), color: null, link: null };

/** The colour an element sets itself, or `null`. */
export function ownColor(element: Element): string | null {
  if (element.nodeName === "FONT") return normalizeColor(element.getAttribute("color") ?? "");
  if (element.nodeName === "SPAN") return colorOfStyle(element.getAttribute("style") ?? "");
  return null;
}

/** The formatting around `node`, read from the markup between it and the editor's root. */
export function caretState(node: Node | null, root: Element): CaretState {
  if (node === null || !root.contains(node)) return EMPTY_CARET;
  const formats = new Set<Format>();
  let color: string | null = null;
  let colorFound = false;
  let link: HTMLAnchorElement | null = null;
  let listFound = false;
  let element: Element | null = node instanceof Element ? node : node.parentElement;
  while (element !== null && element !== root) {
    const format = FORMAT_OF_TAG[element.nodeName];
    if (format !== undefined) formats.add(format);
    if (element instanceof HTMLAnchorElement && link === null) link = element;
    // Only the innermost list counts: a bullet inside a numbered list is a bullet.
    if (!listFound && (element.nodeName === "UL" || element.nodeName === "OL")) {
      formats.add(element.nodeName === "UL" ? "bullets" : "numbers");
      listFound = true;
    }
    if (!colorFound) {
      color = ownColor(element);
      colorFound = color !== null;
    }
    element = element.parentElement;
  }
  return { formats, color, link };
}

/** Replaces an element by its children, keeping the text and any inner formatting. */
export function unwrap(element: Element) {
  element.replaceWith(...Array.from(element.childNodes));
}

/**
 * Removes the text colour from the selection: every coloured element it touches loses its
 * colour (whole elements, so a run is never split into odd fragments).
 */
export function removeColor(range: Range, root: Element) {
  const touched = new Set<Element>();
  const start = range.startContainer;
  let element: Element | null = start instanceof Element ? start : start.parentElement;
  while (element !== null && element !== root) {
    if (ownColor(element) !== null) touched.add(element);
    element = element.parentElement;
  }
  for (const candidate of Array.from(root.querySelectorAll("font, span"))) {
    if (ownColor(candidate) !== null && range.intersectsNode(candidate)) touched.add(candidate);
  }
  touched.forEach(unwrap);
}

/** Escapes text for use inside HTML built here. */
export function escapeHtml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}
