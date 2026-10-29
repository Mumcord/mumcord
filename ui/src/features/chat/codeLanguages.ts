/**
 * The languages a ``` fence can name (issue #112): their aliases, the highlight.js grammar,
 * the label shown in the block's corner and the extension of a downloaded block.
 *
 * Grammars of the auto-detection set are bundled with the chat (detection needs them at once);
 * all others are loaded the first time a message names them, so they cost nothing up front.
 */
import type { LanguageFn } from "highlight.js";

export interface CodeLanguage {
  /** highlight.js grammar; none for plain text and hex, which are handled here. */
  grammar: string | null;
  label: string;
  extension: string;
}

/** Canonical language → how it is shown and saved. */
export const LANGUAGES: Readonly<Record<string, CodeLanguage>> = {
  bash: { grammar: "bash", label: "Bash", extension: "sh" },
  c: { grammar: "c", label: "C", extension: "c" },
  cmd: { grammar: "dos", label: "CMD", extension: "bat" },
  cpp: { grammar: "cpp", label: "C++", extension: "cpp" },
  csharp: { grammar: "csharp", label: "C#", extension: "cs" },
  css: { grammar: "css", label: "CSS", extension: "css" },
  diff: { grammar: "diff", label: "Diff", extension: "diff" },
  dockerfile: { grammar: "dockerfile", label: "Dockerfile", extension: "dockerfile" },
  go: { grammar: "go", label: "Go", extension: "go" },
  hex: { grammar: null, label: "Hex", extension: "hex" },
  html: { grammar: "xml", label: "HTML", extension: "html" },
  ini: { grammar: "ini", label: "INI", extension: "ini" },
  java: { grammar: "java", label: "Java", extension: "java" },
  javascript: { grammar: "javascript", label: "JavaScript", extension: "js" },
  json: { grammar: "json", label: "JSON", extension: "json" },
  kotlin: { grammar: "kotlin", label: "Kotlin", extension: "kt" },
  lua: { grammar: "lua", label: "Lua", extension: "lua" },
  makefile: { grammar: "makefile", label: "Makefile", extension: "mk" },
  markdown: { grammar: "markdown", label: "Markdown", extension: "md" },
  nginx: { grammar: "nginx", label: "nginx", extension: "conf" },
  php: { grammar: "php", label: "PHP", extension: "php" },
  plaintext: { grammar: null, label: "Text", extension: "txt" },
  powershell: { grammar: "powershell", label: "PowerShell", extension: "ps1" },
  python: { grammar: "python", label: "Python", extension: "py" },
  ruby: { grammar: "ruby", label: "Ruby", extension: "rb" },
  rust: { grammar: "rust", label: "Rust", extension: "rs" },
  sql: { grammar: "sql", label: "SQL", extension: "sql" },
  swift: { grammar: "swift", label: "Swift", extension: "swift" },
  toml: { grammar: "ini", label: "TOML", extension: "toml" },
  typescript: { grammar: "typescript", label: "TypeScript", extension: "ts" },
  xml: { grammar: "xml", label: "XML", extension: "xml" },
  yaml: { grammar: "yaml", label: "YAML", extension: "yaml" },
};

const ALIASES: Readonly<Record<string, string>> = {
  sh: "bash",
  shell: "bash",
  zsh: "bash",
  py: "python",
  rs: "rust",
  js: "javascript",
  ts: "typescript",
  "c++": "cpp",
  bat: "cmd",
  batch: "cmd",
  yml: "yaml",
};

/** The canonical language for a fence name (`py` → `python`); `null` when it is unknown. */
export function resolveLanguage(name: string): string | null {
  const lower = name.toLowerCase();
  const canonical = ALIASES[lower] ?? lower;
  return Object.hasOwn(LANGUAGES, canonical) ? canonical : null;
}

/** Grammars outside the auto-detection set, split into their own chunks by the bundler. */
export const LAZY_GRAMMARS: Readonly<Record<string, () => Promise<{ default: LanguageFn }>>> = {
  dockerfile: () => import("highlight.js/lib/languages/dockerfile"),
  dos: () => import("highlight.js/lib/languages/dos"),
  kotlin: () => import("highlight.js/lib/languages/kotlin"),
  lua: () => import("highlight.js/lib/languages/lua"),
  makefile: () => import("highlight.js/lib/languages/makefile"),
  markdown: () => import("highlight.js/lib/languages/markdown"),
  nginx: () => import("highlight.js/lib/languages/nginx"),
  php: () => import("highlight.js/lib/languages/php"),
  powershell: () => import("highlight.js/lib/languages/powershell"),
  ruby: () => import("highlight.js/lib/languages/ruby"),
  swift: () => import("highlight.js/lib/languages/swift"),
};
