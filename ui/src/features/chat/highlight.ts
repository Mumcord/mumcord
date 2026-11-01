/**
 * Syntax highlighting for code in chat messages (FR-1.53, issue #112). Works on HTML that has
 * already been sanitized: only the plain text of each `<code>` element is highlighted, and the
 * highlighter's output is sanitized again so nothing but its own colored `<span>`s can get through.
 *
 * Mumcord sends the language of a ``` fence as a marker on the `<pre>` (`data-mumcord-lang`,
 * see `sanitize.ts`), because Mumble strips the class attribute. A block with the marker is
 * highlighted as exactly that language, or stays plain text when the language is unknown.
 * Code without it (inline code, other Mumble clients) is detected automatically from a curated
 * set of common languages. highlight.js itself lives in `highlighter.ts` and loads with the first
 * message that contains code (issue #323); until then that code is shown plain.
 */
import { useMemo, useSyncExternalStore } from "react";
import { LANGUAGES, resolveLanguage } from "./codeLanguages";
import { CODE_LANGUAGE_ATTRIBUTE } from "./sanitize";

/** Class of the wrapper around every code block; its buttons and label go in there. */
export const CODE_BLOCK_CLASS = "chat-code";

/** The file extension for code in `language`; `txt` when the language is unknown. */
export function extensionFor(language: string | undefined): string {
  return language === undefined ? "txt" : (LANGUAGES[language]?.extension ?? "txt");
}

type Highlighter = typeof import("./highlighter");

// Messages are highlighted again whenever highlight.js or one of its grammars arrives.
let version = 0;
const listeners = new Set<() => void>();
let highlighter: Highlighter | null = null;
let loadingHighlighter: Promise<Highlighter | null> | undefined;

function changed(): void {
  version += 1;
  for (const listener of listeners) listener();
}

/** Loads highlight.js; resolves to `null` (code stays plain) when the chunk cannot be loaded. */
export function loadHighlighter(): Promise<Highlighter | null> {
  loadingHighlighter ??= import("./highlighter").then(
    (module) => {
      module.setOnGrammarLoaded(changed);
      highlighter = module;
      changed();
      return module;
    },
    (error: unknown) => {
      console.error("Loading the code highlighting failed", error);
      return null;
    },
  );
  return loadingHighlighter;
}

/** Loads a grammar outside the auto-detection set; resolves once it can be used. */
export async function loadGrammar(grammar: string): Promise<void> {
  await (await loadHighlighter())?.loadGrammar(grammar);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function colorize(code: Element, html: string | null): void {
  if (html === null) return;
  code.innerHTML = html;
  code.classList.add("hljs");
}

/**
 * Adds syntax colors to every `<code>` in already sanitized message HTML and wraps every `<pre>`
 * in a {@link CODE_BLOCK_CLASS} block. The wrapper keeps the buttons in place while the code
 * scrolls sideways. Its `data-language` (named or detected) names the downloaded file, and
 * `data-language-label` (only when the sender named a language) is shown in the corner.
 */
export function highlightCode(sanitizedHtml: string): string {
  if (!sanitizedHtml.includes("<code") && !sanitizedHtml.includes("<pre")) return sanitizedHtml;
  const template = document.createElement("template");
  template.innerHTML = sanitizedHtml;
  // Without highlight.js yet the blocks get their wrapper and label; colors follow once it loads.
  if (highlighter === null) void loadHighlighter();
  const named = new Set<Element>();
  for (const pre of template.content.querySelectorAll("pre")) {
    const block = document.createElement("div");
    block.className = CODE_BLOCK_CLASS;
    pre.replaceWith(block);
    block.append(pre);
    const name = pre.getAttribute(CODE_LANGUAGE_ATTRIBUTE);
    if (name === null) continue;
    const code = pre.querySelector(":scope > code");
    if (code !== null) named.add(code);
    const language = resolveLanguage(name);
    block.dataset["languageLabel"] =
      language === null ? name : (LANGUAGES[language]?.label ?? name);
    if (language === null) continue;
    block.dataset["language"] = language;
    if (code !== null && highlighter !== null) {
      colorize(code, highlighter.highlightedAs(code.textContent, language));
    }
  }
  if (highlighter === null) return template.innerHTML;
  for (const code of template.content.querySelectorAll("code")) {
    if (named.has(code)) continue;
    const result = highlighter.detected(code.textContent);
    if (result === null) continue;
    colorize(code, result.html);
    const block = code.parentElement?.parentElement;
    if (result.language !== undefined && block?.classList.contains(CODE_BLOCK_CLASS) === true) {
      block.dataset["language"] = result.language;
    }
  }
  return template.innerHTML;
}

/** {@link highlightCode} for a component; runs again when highlight.js or a grammar it waited for has loaded. */
export function useHighlightedCode(sanitizedHtml: string): string {
  const loaded = useSyncExternalStore(subscribe, () => version);
  // `loaded` is not read: it only tells when the same HTML highlights differently now.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(() => highlightCode(sanitizedHtml), [sanitizedHtml, loaded]);
}
