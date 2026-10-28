/**
 * Syntax-highlighted JSON. The text comes from other clients, so it is split into tokens and
 * rendered as plain text nodes; nothing is ever parsed as HTML.
 */
import type { ReactNode } from "react";
import { tokenizeJson } from "./json";

export function JsonView({ json, label }: { json: string; label: string }) {
  const parts: ReactNode[] = tokenizeJson(json).map((token, i) =>
    token.kind === "space" ? (
      token.text
    ) : (
      <span key={i} className={`pi-json-${token.kind}`}>
        {token.text}
      </span>
    ),
  );
  return (
    // Focusable so keyboard users can scroll long payloads.
    // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
    <pre className="pi-code" tabIndex={0} aria-label={label}>
      {parts}
    </pre>
  );
}
