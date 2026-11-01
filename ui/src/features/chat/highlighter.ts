/**
 * The highlight.js part of chat code highlighting (issue #112), split off from `highlight.ts` so
 * highlight.js only loads once a message with code is shown (issue #323). Everything here returns
 * sanitized HTML: only the highlighter's own colored `<span>`s can get through.
 */
import DOMPurify from "dompurify";
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
import { LANGUAGES, LAZY_GRAMMARS } from "./codeLanguages";
import { highlightHex } from "./hexDump";

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

// Grammars loaded on demand: messages are highlighted again once one arrives.
let onGrammarLoaded = (): void => {};
const loading = new Map<string, Promise<void>>();

/** Called whenever a grammar loaded on demand can be used. */
export function setOnGrammarLoaded(listener: () => void): void {
  onGrammarLoaded = listener;
}

/** Loads a grammar outside the auto-detection set; resolves once it can be used. */
export function loadGrammar(grammar: string): Promise<void> {
  const load = LAZY_GRAMMARS[grammar];
  if (load === undefined || hljs.getLanguage(grammar) !== undefined) return Promise.resolve();
  let pending = loading.get(grammar);
  if (pending === undefined) {
    pending = load().then(
      (module) => {
        hljs.registerLanguage(grammar, module.default);
        onGrammarLoaded();
      },
      (error: unknown) => {
        console.error(`Loading the ${grammar} highlighting failed`, error);
      },
    );
    loading.set(grammar, pending);
  }
  return pending;
}

function clean(html: string): string {
  return purifier.sanitize(html, { ALLOWED_TAGS: ["span"], ALLOWED_ATTR: ["class"] });
}

export interface Detected {
  html: string;
  language: string | undefined;
}

/** Highlighted HTML of `text` with an auto-detected language; `null` keeps it plain. */
export function detected(text: string): Detected | null {
  const result = hljs.highlightAuto(text, SUBSET);
  if (result.relevance < MIN_RELEVANCE) return null;
  return { html: clean(result.value), language: result.language };
}

/** Highlighted HTML of `text` in a known language; `null` keeps it plain. */
export function highlightedAs(text: string, language: string): string | null {
  if (language === "hex") return clean(highlightHex(text));
  const grammar = LANGUAGES[language]?.grammar ?? null;
  if (grammar === null) return null;
  if (hljs.getLanguage(grammar) === undefined) {
    // Shown plain for now; the grammar's arrival re-renders it highlighted.
    void loadGrammar(grammar);
    return null;
  }
  return clean(hljs.highlight(text, { language: grammar, ignoreIllegals: true }).value);
}
