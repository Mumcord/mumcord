/**
 * Chat replies (FR-2.60). A reply is a normal Mumble text message whose HTML starts with a short
 * quote of the original, so official clients show it as readable text:
 * - HTML servers: `<blockquote><b>Name</b>: snippet</blockquote>reply`
 * - plain-text servers: `> Name: snippet` on its own line, then the reply
 * Mumcord recognises both and shows a compact reply header instead. The quote comes from other
 * users and is untrusted: it is only ever read and shown as plain text, with capped lengths.
 */
import type { ChatMessage } from "../../api";
import { escapeHtml } from "./markdown";
import { htmlToText, sanitizeHtml } from "./sanitize";

/** Characters of the original a reply quotes. */
export const SNIPPET_CHARS = 100;
/** Longest sender name kept from a quote; Mumble names are much shorter. */
const NAME_CHARS = 64;
const ELLIPSIS = "…";

export interface ReplyQuote {
  name: string;
  snippet: string;
}

export interface ParsedReply {
  quote: ReplyQuote;
  /** The sanitized message HTML without the quote: the reply itself. */
  rest: string;
}

function collapse(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

function capped(name: string, snippet: string): ReplyQuote {
  return {
    name: collapse(name).slice(0, NAME_CHARS),
    snippet: collapse(snippet).slice(0, SNIPPET_CHARS + ELLIPSIS.length),
  };
}

/** The quote that starts a reply message, built from the original's sender and text. */
export function quoteHtml(quote: ReplyQuote, allowHtml: boolean): string {
  const name = escapeHtml(quote.name);
  const snippet = escapeHtml(quote.snippet);
  return allowHtml
    ? `<blockquote><b>${name}</b>: ${snippet}</blockquote>`
    : `&gt; ${name}: ${snippet}<br>`;
}

/** Shortens a message's text to what a reply quotes. */
export function snippet(text: string): string {
  const flat = collapse(text);
  return flat.length > SNIPPET_CHARS
    ? `${flat.slice(0, SNIPPET_CHARS).trimEnd()}${ELLIPSIS}`
    : flat;
}

// `<blockquote><b>Name</b>: snippet</blockquote>` as the very first node.
function htmlQuote(body: HTMLElement): ReplyQuote | null {
  const first = body.firstChild;
  if (!(first instanceof HTMLQuoteElement) || first.tagName !== "BLOCKQUOTE") return null;
  const bold = first.firstChild;
  if (!(bold instanceof HTMLElement) || bold.tagName !== "B") return null;
  const after = bold.nextSibling;
  if (!(after instanceof Text) || !after.data.startsWith(": ")) return null;
  const name = bold.textContent;
  const quoted = first.textContent.slice(name.length + 2);
  first.remove();
  return capped(name, quoted);
}

// `> Name: snippet` as the first line of plain text, ended by a line break.
function plainQuote(body: HTMLElement): ReplyQuote | null {
  const first = body.firstChild;
  if (!(first instanceof Text) || !first.data.startsWith("> ")) return null;
  const newline = first.data.indexOf("\n");
  let line: string;
  if (newline >= 0) {
    line = first.data.slice(2, newline);
    first.data = first.data.slice(newline + 1);
  } else if (first.nextSibling instanceof HTMLBRElement) {
    line = first.data.slice(2);
    first.nextSibling.remove();
    first.remove();
  } else {
    return null;
  }
  const colon = line.indexOf(": ");
  if (colon <= 0) return null;
  return capped(line.slice(0, colon), line.slice(colon + 2));
}

/** The quote and the rest of a reply message, or `null` when the message is no reply. */
export function parseReply(sanitized: string): ParsedReply | null {
  // Sanitizing leaves text without any tags as it is, so a plain `>` may arrive unescaped.
  if (!/^(?:<blockquote>|&gt; |> )/.test(sanitized)) return null;
  const doc = new DOMParser().parseFromString(sanitized, "text/html");
  const quote = htmlQuote(doc.body) ?? plainQuote(doc.body);
  if (quote === null) return null;
  return { quote, rest: doc.body.innerHTML };
}

// Messages are immutable objects in the store, so their reply parts are worked out only once.
const parsed = new WeakMap<ChatMessage, ParsedReply | null>();
const texts = new WeakMap<ChatMessage, string>();

/** The reply quote of a message, if it is a reply. */
export function replyOf(message: ChatMessage): ParsedReply | null {
  let reply = parsed.get(message);
  if (reply === undefined) {
    reply = parseReply(sanitizeHtml(message.body));
    parsed.set(message, reply);
  }
  return reply;
}

/** The visible text of a message without its own reply quote. */
export function messageText(message: ChatMessage): string {
  let text = texts.get(message);
  if (text === undefined) {
    const reply = replyOf(message);
    text = collapse(htmlToText(reply ? reply.rest : message.body));
    texts.set(message, text);
  }
  return text;
}

/**
 * The quote for replying to `message`; `name` is the sender as shown, `imageLabel` stands in for
 * a message without text (an image).
 */
export function quoteFor(message: ChatMessage, name: string, imageLabel: string): ReplyQuote {
  return capped(name, snippet(messageText(message)) || imageLabel);
}

// The start of the original's text a quote shows, without the ellipsis of a shortened snippet.
function quotedText(quote: ReplyQuote): string {
  return collapse(
    quote.snippet.endsWith(ELLIPSIS) ? quote.snippet.slice(0, -ELLIPSIS.length) : quote.snippet,
  );
}

/**
 * The message a reply quotes: the latest one before `before` from the quoted sender whose text
 * starts with the quote. `nameAt` gives the sender as shown of the message at an index.
 */
export function findOriginal(
  messages: readonly ChatMessage[],
  before: number,
  quote: ReplyQuote,
  nameAt: (index: number) => string,
  imageLabel: string,
): number | null {
  const wanted = quotedText(quote);
  if (wanted === "") return null;
  for (let index = Math.min(before, messages.length) - 1; index >= 0; index--) {
    const message = messages[index];
    if (message === undefined || collapse(nameAt(index)).slice(0, NAME_CHARS) !== quote.name) {
      continue;
    }
    const text = messageText(message);
    if (text === "" ? wanted === imageLabel : text.startsWith(wanted)) return index;
  }
  return null;
}

/**
 * Whether `message` replies to one of the user's own messages (FR-2.67): its quote names the user
 * by `ownName`, their current name. When the quoted original is in `history` (it starts with the
 * snippet and is the user's own or sent under that name), it must be one of the user's own;
 * someone else who had the name earlier does not count. Without the original the name decides.
 */
export function repliesToOwn(
  message: ChatMessage,
  ownName: string,
  history: readonly ChatMessage[],
): boolean {
  if (message.own) return false;
  const quote = replyOf(message)?.quote;
  if (quote === undefined || quote.name !== collapse(ownName).slice(0, NAME_CHARS)) return false;
  const wanted = quotedText(quote);
  if (wanted === "") return true;
  let othersOriginal = false;
  for (const candidate of history) {
    if (candidate.id === message.id) continue;
    if (!candidate.own && collapse(candidate.senderName).slice(0, NAME_CHARS) !== quote.name) {
      continue;
    }
    if (!messageText(candidate).startsWith(wanted)) continue;
    if (candidate.own) return true;
    othersOriginal = true;
  }
  return !othersOriginal;
}
