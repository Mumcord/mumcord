/**
 * Syntax highlighting for code in chat messages (FR-1.53, issue #112). Works on HTML that has
 * already been sanitized: only the plain text of each `<code>` element is highlighted, and the
 * highlighter's output is sanitized again so nothing but its own colored `<span>`s can get through.
 *
 * Mumcord sends the language of a ``` fence as a marker on the `<pre>` (`data-mumcord-lang`,
 * see `sanitize.ts`), because Mumble strips the class attribute. A block with the marker is
 * highlighted as exactly that language, or stays plain text when the language is unknown.
 * Code without it (inline code, other Mumble clients) is detected automatically from a curated
 * set of common languages.
 */
import DOMPurify from "dompurify";
import { useMemo, useSyncExternalStore } from "react";
import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import csharp from "highlight.js/lib/languages/csharp";
import css from "highlight.js/lib/languages/css";
import diff from "highlight.js/lib/languages/diff";
import go from "highlight.js/lib/languages/go";
import ini from "highlight.js/lib/languages/ini";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import python from "highlight.js/lib/languages/python";
import rust from "highlight.js/lib/languages/rust";
import sql from "highlight.js/lib/languages/sql";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";
import { LANGUAGES, LAZY_GRAMMARS, resolveLanguage } from "./codeLanguages";
import { highlightHex } from "./hexDump";
import { CODE_LANGUAGE_ATTRIBUTE } from "./sanitize";

/** The auto-detection set; bundled because detection needs every grammar at once. */
const DETECTED = {
  bash,
  c,
  cpp,
  csharp,
  css,
  diff,
  go,
  ini,
  java,
  javascript,
  json,
  python,
  rust,
  sql,
  typescript,
  xml,
  yaml,
};

for (const [name, language] of Object.entries(DETECTED)) hljs.registerLanguage(name, language);
const SUBSET = Object.keys(DETECTED);

/**
 * Below this relevance the guess is noise (a word or two of inline code); the text stays plain.
 * highlight.js scores roughly one point per recognised keyword or construct.
 */
const MIN_RELEVANCE = 2;

/** highlight.js scope classes, e.g. `hljs-keyword` or the `function_` modifier of a title. */
const SCOPE_CLASS = /^(?:hljs-[a-z-]+|[a-z]+_+)$/;

const purifier = DOMPurify(window);
purifier.addHook("uponSanitizeAttribute", (_node, data) => {
  if (data.attrName !== "class") return;
  const kept = data.attrValue.split(/\s+/).filter((name) => SCOPE_CLASS.test(name));
  data.attrValue = kept.join(" ");
  if (kept.length === 0) data.keepAttr = false;
});

/** Class of the wrapper around every code block; its buttons and label go in there. */
export const CODE_BLOCK_CLASS = "chat-code";

/** The file extension for code in `language`; `txt` when the language is unknown. */
export function extensionFor(language: string | undefined): string {
  return language === undefined ? "txt" : (LANGUAGES[language]?.extension ?? "txt");
}

// Grammars loaded on demand: messages are highlighted again once one arrives.
let version = 0;
const listeners = new Set<() => void>();
const loading = new Map<string, Promise<void>>();

/** Loads a grammar outside the auto-detection set; resolves once it can be used. */
export function loadGrammar(grammar: string): Promise<void> {
  const load = LAZY_GRAMMARS[grammar];
  if (load === undefined || hljs.getLanguage(grammar) !== undefined) return Promise.resolve();
  let pending = loading.get(grammar);
  if (pending === undefined) {
    pending = load().then(
      (module) => {
        hljs.registerLanguage(grammar, module.default);
        version += 1;
        for (const listener of listeners) listener();
      },
      (error: unknown) => {
        console.error(`Loading the ${grammar} highlighting failed`, error);
      },
    );
    loading.set(grammar, pending);
  }
  return pending;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function clean(html: string): string {
  return purifier.sanitize(html, { ALLOWED_TAGS: ["span"], ALLOWED_ATTR: ["class"] });
}

interface Detected {
  html: string;
  language: string | undefined;
}

function detected(text: string): Detected | null {
  const result = hljs.highlightAuto(text, SUBSET);
  if (result.relevance < MIN_RELEVANCE) return null;
  return { html: clean(result.value), language: result.language };
}

/** Highlighted HTML of `text` in a known language; `null` keeps it plain. */
function highlightedAs(text: string, language: string): string | null {
  if (language === "hex") return clean(highlightHex(text));
  const grammar = LANGUAGES[language]?.grammar ?? null;
  if (grammar === null) return null;
  if (hljs.getLanguage(grammar) === undefined) {
    // Shown plain for now; the version bump re-renders it highlighted.
    void loadGrammar(grammar);
    return null;
  }
  return clean(hljs.highlight(text, { language: grammar, ignoreIllegals: true }).value);
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
    if (code !== null) colorize(code, highlightedAs(code.textContent, language));
  }
  for (const code of template.content.querySelectorAll("code")) {
    if (named.has(code)) continue;
    const result = detected(code.textContent);
    if (result === null) continue;
    colorize(code, result.html);
    const block = code.parentElement?.parentElement;
    if (result.language !== undefined && block?.classList.contains(CODE_BLOCK_CLASS) === true) {
      block.dataset["language"] = result.language;
    }
  }
  return template.innerHTML;
}

/** {@link highlightCode} for a component; runs again when a grammar it waited for has loaded. */
export function useHighlightedCode(sanitizedHtml: string): string {
  const loaded = useSyncExternalStore(subscribe, () => version);
  // `loaded` is not read: it only tells when the same HTML highlights differently now.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(() => highlightCode(sanitizedHtml), [sanitizedHtml, loaded]);
}
