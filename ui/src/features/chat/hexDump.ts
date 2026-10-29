/**
 * Colors for ```hex blocks (issue #112); highlight.js has no grammar for hex dumps. Understands
 * `hexdump -C` (offset, bytes, `|ASCII|`), `xxd` (`offset:`, byte groups, ASCII after two spaces)
 * and free-form hex (`48 65`, `0x48, 0x65`, `\x48\x65`, `48:65`). Anything it does not recognise
 * stays plain text, so odd input never breaks the block.
 *
 * The output uses highlight.js scope classes, so it gets the theme's syntax colors and passes the
 * same sanitizing as highlight.js output.
 */
import { escapeHtml } from "./markdown";

/** `00000010:` (xxd), `0x0010:` or the 8-digit offset of `hexdump -C`, which needs two spaces. */
const OFFSET = /^(\s*)((?:0x)?[0-9a-f]{4,16}:|[0-9a-f]{8}(?= {2}))/i;
/** The `|ASCII|` column of `hexdump -C`. */
const PIPE_ASCII = /(\s+)(\|.*\|)(\s*)$/;
/** A byte token: hex digit pairs with an optional `0x` or `\x` prefix. */
const BYTES = /^(0x|\\x)?((?:[0-9a-f]{2})+)$/i;

function span(scope: string, text: string): string {
  return `<span class="hljs-${scope}">${escapeHtml(text)}</span>`;
}

function bytes(token: string): string | null {
  const match = BYTES.exec(token);
  if (match === null) return null;
  const prefix = match[1] === undefined ? "" : span("comment", match[1]);
  const pairs = (match[2] ?? "").match(/../g) ?? [];
  // Zero bytes are dimmed: padding and empty fields stand out from the data.
  return prefix + pairs.map((pair) => span(pair === "00" ? "comment" : "number", pair)).join("");
}

/** Bytes separated by spaces, commas, colons or nothing (`\x48\x65`). */
function byteRun(text: string): string {
  return (text.match(/\\x[0-9a-f]{2}|[^\s,:]+|[\s,:]+/gi) ?? [])
    .map((token) => bytes(token) ?? escapeHtml(token))
    .join("");
}

function line(text: string): string {
  let rest = text;
  let html = "";
  const offset = OFFSET.exec(rest);
  const xxd = offset?.[2]?.endsWith(":") === true;
  if (offset !== null && rest.length > offset[0].length) {
    html += escapeHtml(offset[1] ?? "") + span("comment", offset[2] ?? "");
    rest = rest.slice(offset[0].length);
  }
  let ascii = "";
  const pipe = PIPE_ASCII.exec(rest);
  if (pipe !== null) {
    ascii = escapeHtml(pipe[1] ?? "") + span("string", pipe[2] ?? "") + escapeHtml(pipe[3] ?? "");
    rest = rest.slice(0, pipe.index);
  } else if (xxd) {
    // xxd puts the text column after two spaces; the byte groups are one space apart.
    const column = /^(\s*\S+(?: \S+)*)( {2,})(.+)$/.exec(rest);
    if (column !== null) {
      ascii = escapeHtml(column[2] ?? "") + span("string", column[3] ?? "");
      rest = column[1] ?? "";
    }
  }
  return html + byteRun(rest) + ascii;
}

/** Highlighted HTML for a hex dump; every character of `text` is kept. */
export function highlightHex(text: string): string {
  return text.split("\n").map(line).join("\n");
}
