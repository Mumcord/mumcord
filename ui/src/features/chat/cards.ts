/**
 * Message cards (issue #72, docs/message-cards.md): the BB code
 * `[card title="…" color=… icon=… image="…" url="…"]text[/card]`, its wire format and the model
 * the UI renders.
 *
 * On the wire a card is ordinary chat HTML that reads well in official Mumble clients (bold title
 * with the status as `[Warning]`, the text, the image and the link as plain links), wrapped in a
 * `<div data-mumcord-card="…">` whose value carries the fields URL-encoded. The text sits in an
 * inner `<div data-mumcord-card="body">`. Official clients ignore the attribute; Mumcord renders
 * the card from it. Typed BB code that arrives as text (e.g. from an official client) becomes a
 * card, too.
 *
 * Everything here is untrusted server input: values are parsed strictly and anything that does
 * not fit is dropped; a card without a title and a text stays normal chat text.
 */
import { escapeHtml } from "./markdown";
import { sanitizeHtml } from "./sanitize";

export const CARD_COLORS = ["info", "success", "warning", "error"] as const;
export type CardColor = (typeof CARD_COLORS)[number];

export const CARD_ICONS = [
  "info",
  "check",
  "warning",
  "error",
  "bell",
  "chart",
  "server",
  "bot",
] as const;
export type CardIcon = (typeof CARD_ICONS)[number];

export interface MessageCard {
  title: string | null;
  color: CardColor | null;
  icon: CardIcon | null;
  /** http(s) URL; only fetched by the engine when link previews are on. */
  image: string | null;
  /** http(s) URL of the link line and the Open button. */
  url: string | null;
  /** Chat HTML of the text, sanitized; empty when the card has none. */
  body: string;
}

/** The attribute that marks a card on the wire; its value `body` marks the card's text. */
export const CARD_ATTRIBUTE = "data-mumcord-card";
const BODY_VALUE = "body";
const WIRE_VERSION = "1";

/** Status labels in the fallback HTML; the receiving official client cannot translate them. */
const FALLBACK_LABEL: Record<CardColor, string> = {
  info: "Info",
  success: "Success",
  warning: "Warning",
  error: "Error",
};

const WEB_URL = /^https?:\/\/[^\s"<>]+$/i;

function webUrl(value: string | undefined | null): string | null {
  const url = value?.trim() ?? "";
  if (!WEB_URL.test(url)) return null;
  try {
    const parsed = new URL(url);
    return parsed.host === "" ? null : url;
  } catch {
    return null;
  }
}

function oneOf<T extends string>(values: readonly T[], value: string | undefined): T | null {
  const lower = value?.trim().toLowerCase();
  return values.find((v) => v === lower) ?? null;
}

/** The card's fields from raw attribute values; unknown and invalid ones are dropped. */
function fields(get: (name: string) => string | undefined, body: string): MessageCard | null {
  const title = get("title")?.replace(/\s+/g, " ").trim() ?? "";
  const card: MessageCard = {
    title: title === "" ? null : title,
    color: oneOf(CARD_COLORS, get("color")),
    icon: oneOf(CARD_ICONS, get("icon")),
    image: webUrl(get("image")),
    url: webUrl(get("url")),
    body,
  };
  return card.title === null && isBlankHtml(body) ? null : card;
}

function isBlankHtml(html: string): boolean {
  return html.replace(/<br\s*\/?>|&nbsp;|\s/gi, "") === "";
}

/** One attribute of the opening tag: `name=value` or `name="value with spaces"`. */
const ATTRIBUTE = String.raw`\s+[A-Za-z][\w-]*=(?:"[^"]*"|[^\s"\]]+)`;
const OPENING = String.raw`\[card((?:${ATTRIBUTE})*)\s*\]`;
const CLOSING = String.raw`\[\/card\]`;
const ATTRIBUTE_PARTS = /([A-Za-z][\w-]*)=(?:"([^"]*)"|([^\s"\]]+))/g;

function bbCodeBlocks(): RegExp {
  return new RegExp(`${OPENING}([\\s\\S]*?)${CLOSING}`, "gi");
}

function parseAttributes(source: string, decode: (value: string) => string): Map<string, string> {
  const values = new Map<string, string>();
  for (const match of source.matchAll(ATTRIBUTE_PARTS)) {
    const name = (match[1] ?? "").toLowerCase();
    // The first value wins; a repeated attribute cannot override what a reader saw first.
    if (!values.has(name)) values.set(name, decode(match[2] ?? match[3] ?? ""));
  }
  return values;
}

export type CardPart = { kind: "text"; text: string } | { kind: "card"; card: MessageCard };

/**
 * Splits typed text into plain text and cards. The card text stays as typed (Markdown); it is
 * converted by the caller. A broken or empty card stays plain text.
 */
export function splitCardSource(source: string): CardPart[] {
  const parts: CardPart[] = [];
  let at = 0;
  for (const match of source.matchAll(bbCodeBlocks())) {
    const attributes = parseAttributes(match[1] ?? "", (value) => value);
    const text = (match[2] ?? "").trim();
    const card = fields((name) => attributes.get(name), text);
    if (card === null) continue;
    if (match.index > at) parts.push({ kind: "text", text: source.slice(at, match.index) });
    parts.push({ kind: "card", card });
    at = match.index + match[0].length;
  }
  if (at < source.length) parts.push({ kind: "text", text: source.slice(at) });
  return parts;
}

/** The text a link shows for a URL: host and path, without the scheme. */
export function displayUrl(url: string): string {
  return url.replace(/^https?:\/\//i, "").replace(/\/$/, "");
}

/** The wire HTML of a card; `body` must already be safe chat HTML. */
export function cardToHtml(card: MessageCard): string {
  const params = new URLSearchParams({ v: WIRE_VERSION });
  if (card.title !== null) params.set("title", card.title);
  if (card.color !== null) params.set("color", card.color);
  if (card.icon !== null) params.set("icon", card.icon);
  if (card.image !== null) params.set("image", card.image);
  if (card.url !== null) params.set("url", card.url);
  const label = card.color === null ? "" : `[${FALLBACK_LABEL[card.color]}]`;
  const heading = [label, card.title === null ? "" : escapeHtml(card.title)]
    .filter((part) => part !== "")
    .join(" ");
  const lines: string[] = [];
  if (card.image !== null) {
    lines.push(
      `Image: <a href="${escapeHtml(card.image)}">${escapeHtml(displayUrl(card.image))}</a>`,
    );
  }
  if (card.url !== null) {
    lines.push(`<a href="${escapeHtml(card.url)}">${escapeHtml(card.url)}</a>`);
  }
  return (
    `<div ${CARD_ATTRIBUTE}="${escapeHtml(params.toString())}">` +
    (heading === "" ? "" : `<b>${heading}</b>`) +
    (isBlankHtml(card.body) ? "" : `<div ${CARD_ATTRIBUTE}="${BODY_VALUE}">${card.body}</div>`) +
    lines.join("<br>") +
    "</div>"
  );
}

/**
 * Typed text → message HTML with cards in wire format, or `null` when the text has no card.
 * `toHtml` converts the typed Markdown of the text around and inside the cards.
 */
export function composeCards(source: string, toHtml: (text: string) => string): string | null {
  const parts = splitCardSource(source);
  if (!parts.some((part) => part.kind === "card")) return null;
  return parts
    .map((part) =>
      part.kind === "card"
        ? cardToHtml({ ...part.card, body: toHtml(part.card.body) })
        : toHtml(part.text),
    )
    .join("");
}

function decodeEntities(text: string): string {
  return new DOMParser().parseFromString(`<body>${text}`, "text/html").body.textContent;
}

/** Leading and trailing line breaks left around a card typed in an official client. */
const EDGE_BREAKS = /^(?:\s|<br\s*\/?>)+|(?:\s|<br\s*\/?>)+$/gi;

/**
 * Takes the cards out of sanitized message HTML: wire-format cards and typed BB code. Returns the
 * rest of the message (sanitized again) and the cards in message order.
 */
export function extractCards(html: string): { rest: string; cards: MessageCard[] } {
  if (!html.includes(CARD_ATTRIBUTE) && !/\[card/i.test(html)) return { rest: html, cards: [] };
  const doc = new DOMParser().parseFromString(`<body>${html}`, "text/html");
  const cards: MessageCard[] = [];
  for (const element of doc.body.querySelectorAll(`div[${CARD_ATTRIBUTE}]`)) {
    const value = element.getAttribute(CARD_ATTRIBUTE) ?? "";
    if (value === BODY_VALUE || element.parentElement?.closest(`[${CARD_ATTRIBUTE}]`)) continue;
    const params = new URLSearchParams(value);
    const body = Array.from(element.children).find(
      (child) => child.tagName === "DIV" && child.getAttribute(CARD_ATTRIBUTE) === BODY_VALUE,
    );
    const card = fields((name) => params.get(name) ?? undefined, body?.innerHTML.trim() ?? "");
    if (card === null) continue;
    cards.push(card);
    element.remove();
  }
  const wireCards = cards.length;
  const rest = doc.body.innerHTML.replace(
    bbCodeBlocks(),
    (whole, attributes: string, inner: string) => {
      const values = parseAttributes(attributes, decodeEntities);
      const body = sanitizeHtml(inner.replace(EDGE_BREAKS, ""));
      const card = fields((name) => values.get(name), body);
      if (card === null) return whole;
      cards.push(card);
      return "";
    },
  );
  if (cards.length === 0) return { rest: html, cards };
  // Cutting typed BB code out of the HTML string can leave broken markup: sanitize it again.
  return {
    rest: cards.length > wireCards ? sanitizeHtml(rest.replace(EDGE_BREAKS, "")) : rest.trim(),
    cards,
  };
}
