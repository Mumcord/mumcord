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

/** Class of the wrapper around every code block; its copy and download buttons go in there. */
export const CODE_BLOCK_CLASS = "chat-code";

/** File extensions of the highlighted languages, for downloading a block. */
const EXTENSIONS: Record<string, string> = {
  bash: "sh",
  c: "c",
  cpp: "cpp",
  csharp: "cs",
  css: "css",
  diff: "diff",
  go: "go",
  ini: "ini",
  java: "java",
  javascript: "js",
  json: "json",
  python: "py",
  rust: "rs",
  sql: "sql",
  typescript: "ts",
  xml: "xml",
  yaml: "yaml",
};

/** The file extension for code in `language`; `txt` when the language is unknown. */
export function extensionFor(language: string | undefined): string {
  return (language !== undefined && EXTENSIONS[language]) || "txt";
}

interface Highlighted {
  html: string;
  language: string | undefined;
}

function highlighted(text: string): Highlighted | null {
  const result = hljs.highlightAuto(text, SUBSET);
  if (result.relevance < MIN_RELEVANCE) return null;
  const html = purifier.sanitize(result.value, { ALLOWED_TAGS: ["span"], ALLOWED_ATTR: ["class"] });
  return { html, language: result.language };
}

/**
 * Adds syntax colors to every `<code>` in already sanitized message HTML and wraps every `<pre>`
 * in a {@link CODE_BLOCK_CLASS} block. The wrapper keeps the buttons in place while the code
 * scrolls sideways; its `data-language` (the detected language) names the downloaded file.
 */
export function highlightCode(sanitizedHtml: string): string {
  if (!sanitizedHtml.includes("<code") && !sanitizedHtml.includes("<pre")) return sanitizedHtml;
  const template = document.createElement("template");
  template.innerHTML = sanitizedHtml;
  const languages = new Map<Element, string>();
  for (const code of template.content.querySelectorAll("code")) {
    const result = highlighted(code.textContent);
    if (result === null) continue;
    code.innerHTML = result.html;
    code.classList.add("hljs");
    if (result.language !== undefined) languages.set(code, result.language);
  }
  for (const pre of template.content.querySelectorAll("pre")) {
    const block = document.createElement("div");
    block.className = CODE_BLOCK_CLASS;
    const code = pre.querySelector(":scope > code");
    const language = code === null ? undefined : languages.get(code);
    if (language !== undefined) block.dataset["language"] = language;
    pre.replaceWith(block);
    block.append(pre);
  }
  return template.innerHTML;
}
