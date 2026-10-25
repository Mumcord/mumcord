/**
 * Syntax highlighting for code in chat messages (FR-1.53). Works on HTML that has already been
 * sanitized: only the plain text of each `<code>` element is highlighted, and the highlighter's
 * output is sanitized again so nothing but its own colored `<span>`s can get through.
 *
 * The language tag of a ``` fence does not survive the trip through the server (the class
 * attribute is stripped everywhere, and other Mumble clients never send one), so the language is
 * detected automatically from a curated set of common languages.
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

const LANGUAGES = {
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

for (const [name, language] of Object.entries(LANGUAGES)) hljs.registerLanguage(name, language);
const SUBSET = Object.keys(LANGUAGES);

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

function highlighted(text: string): string | null {
  const result = hljs.highlightAuto(text, SUBSET);
  if (result.relevance < MIN_RELEVANCE) return null;
  return purifier.sanitize(result.value, { ALLOWED_TAGS: ["span"], ALLOWED_ATTR: ["class"] });
}

/** Adds syntax colors to every `<code>` in already sanitized message HTML. */
export function highlightCode(sanitizedHtml: string): string {
  if (!sanitizedHtml.includes("<code")) return sanitizedHtml;
  const template = document.createElement("template");
  template.innerHTML = sanitizedHtml;
  for (const code of template.content.querySelectorAll("code")) {
    const html = highlighted(code.textContent);
    if (html === null) continue;
    code.innerHTML = html;
    code.classList.add("hljs");
  }
  return template.innerHTML;
}
