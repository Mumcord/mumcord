/**
 * Data and decisions of the "What's new" window (issue #145). The changelog itself is built from
 * CHANGELOG.md at build time and served as changelog.json (see vite.config.ts).
 */
import { Marked } from "marked";
import { escapeHtml } from "../chat/markdown";
import { sanitizeHtml } from "../chat/sanitize";
import type { ChangelogVersion } from "./parseChangelog";

export type { ChangelogSection, ChangelogVersion } from "./parseChangelog";

const ISSUES_URL = "https://github.com/Mumcord/mumcord/issues/";

/** Groups meant for developers; collapsed until opened. */
export const INTERNAL = "Internal";

export async function loadChangelog(): Promise<ChangelogVersion[]> {
  const response = await fetch(`${import.meta.env.BASE_URL}changelog.json`);
  if (!response.ok) throw new Error(`HTTP ${String(response.status)}`);
  return (await response.json()) as ChangelogVersion[];
}

/** Builds other than releases show `dev~…` (src-tauri/src/version.rs). */
export function isDevBuild(version: string): boolean {
  return version.startsWith("dev~");
}

/** The versions a build shows: work in progress only in dev builds. */
export function visibleVersions(
  versions: readonly ChangelogVersion[],
  appVersion: string,
): ChangelogVersion[] {
  return isDevBuild(appVersion) ? [...versions] : versions.filter((v) => !v.unreleased);
}

/**
 * The version this build is: the running release, or for a dev build the newest entry (the one
 * in development). Falls back to the newest entry for a release missing from the changelog.
 */
export function currentVersion(
  versions: readonly ChangelogVersion[],
  appVersion: string,
): string | null {
  if (!isDevBuild(appVersion) && versions.some((v) => v.version === appVersion)) return appVersion;
  return versions[0]?.version ?? null;
}

/** The look of a group: one of the known kinds, everything else as "other". */
export type GroupKind = "added" | "changed" | "fixed" | "removed" | "internal" | "other";

export function groupKind(title: string): GroupKind {
  switch (title.toLowerCase()) {
    case "added":
      return "added";
    case "changed":
      return "changed";
    case "fixed":
    case "fixes":
      return "fixed";
    case "removed":
      return "removed";
    case "internal":
      return "internal";
    default:
      return "other";
  }
}

/**
 * The one-line summary of a collapsed version: its own summary paragraph, otherwise the first
 * sentence of its first user-facing item, as plain text.
 */
export function summaryOf(version: ChangelogVersion): string | null {
  if (version.summary !== undefined) return plainText(version.summary);
  const first = version.sections.find((s) => s.title !== INTERNAL)?.items[0];
  if (first === undefined) return null;
  const text = plainText(first);
  const sentence = /^.*?[.:;](?=\s|$)/.exec(text)?.[0] ?? text;
  return sentence.replace(/[:;]$/, "");
}

/** Markdown without its markup, for summaries and search. */
export function plainText(markdown: string): string {
  return markdown
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[`*_]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * The versions and items matching `query` (any case); versions whose number or summary match
 * keep all items. An empty query keeps everything.
 */
export function filterChangelog(
  versions: readonly ChangelogVersion[],
  query: string,
): ChangelogVersion[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return [...versions];
  const matches = (text: string) => plainText(text).toLowerCase().includes(needle);
  const result: ChangelogVersion[] = [];
  for (const version of versions) {
    if (version.version.toLowerCase().includes(needle) || matches(version.summary ?? "")) {
      result.push(version);
      continue;
    }
    const sections = version.sections
      .map((s) => ({ title: s.title, items: s.items.filter(matches) }))
      .filter((s) => s.items.length > 0);
    if (sections.length > 0) result.push({ ...version, sections });
  }
  return result;
}

export interface WhatsNewState {
  /** The running build's display version. */
  version: string;
  wizardCompleted: boolean;
  whatsNewAuto: boolean;
  whatsNewSeen: string | null;
}

/**
 * What to do on start: open the window once per new release and remember the version. Dev
 * builds never open it, and a first start (wizard not finished yet) only remembers the version,
 * since a fresh install is not an update.
 */
export function whatsNewOnStart(state: WhatsNewState): { open: boolean; remember: boolean } {
  if (isDevBuild(state.version) || state.whatsNewSeen === state.version) {
    return { open: false, remember: false };
  }
  return { open: state.whatsNewAuto && state.wizardCompleted, remember: true };
}

const marked = new Marked({
  gfm: true,
  renderer: {
    html({ text }) {
      return escapeHtml(text);
    },
  },
});

/** One changelog entry as sanitized HTML; links are opened through the link menu. */
export function itemHtml(markdown: string): string {
  return linkIssues(sanitizeHtml(marked.parse(markdown, { async: false }).trim()));
}

/**
 * Turns `#123` in the text into a link to the GitHub issue or pull request (GitHub redirects
 * either way); text inside links and code stays as it is.
 */
function linkIssues(html: string): string {
  const template = document.createElement("template");
  template.innerHTML = html;
  const walker = document.createTreeWalker(template.content, NodeFilter.SHOW_TEXT);
  const texts: Text[] = [];
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    if (node instanceof Text && node.parentElement?.closest("a, code") == null) texts.push(node);
  }
  for (const text of texts) {
    // Split with a capture group: even parts are text, odd parts issue numbers.
    const parts = text.data.split(/(?<![\w&/])#(\d+)\b/u);
    if (parts.length === 1) continue;
    const fragment = document.createDocumentFragment();
    parts.forEach((part, index) => {
      if (index % 2 === 0) {
        fragment.append(part);
        return;
      }
      const link = document.createElement("a");
      link.href = `${ISSUES_URL}${part}`;
      link.rel = "noopener noreferrer";
      link.textContent = `#${part}`;
      fragment.append(link);
    });
    text.replaceWith(fragment);
  }
  return template.innerHTML;
}
