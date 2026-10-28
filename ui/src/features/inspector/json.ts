/** Splits pretty-printed JSON into tokens for highlighting; never interprets it as HTML. */
export type TokenKind = "key" | "string" | "number" | "literal" | "punctuation" | "space";

const TOKEN =
  /("(?:[^"\\]|\\.)*")(\s*:)?|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|(true|false|null)|([{}[\],:])|(\s+)|(.)/gsy;

export function tokenizeJson(text: string): { kind: TokenKind; text: string }[] {
  const tokens: { kind: TokenKind; text: string }[] = [];
  TOKEN.lastIndex = 0;
  for (const m of text.matchAll(TOKEN)) {
    const [, str, colon, num, literal, punct, space, other] = m;
    if (str !== undefined) {
      tokens.push({ kind: colon === undefined ? "string" : "key", text: str });
      if (colon !== undefined) tokens.push({ kind: "punctuation", text: colon });
    } else if (num !== undefined) tokens.push({ kind: "number", text: num });
    else if (literal !== undefined) tokens.push({ kind: "literal", text: literal });
    else if (punct !== undefined) tokens.push({ kind: "punctuation", text: punct });
    else if (space !== undefined) tokens.push({ kind: "space", text: space });
    else if (other !== undefined) tokens.push({ kind: "punctuation", text: other });
  }
  return tokens;
}
