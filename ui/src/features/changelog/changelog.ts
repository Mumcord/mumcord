/**
 * Data and decisions of the "What's new" window (issue #145). The changelog itself is built from
 * CHANGELOG.md at build time and bundled as the module `virtual:changelog` (see vite.config.ts).
 */
import { Marked } from "marked";
import changelog from "virtual:changelog";
import { escapeHtml } from "../chat/markdown";
import { sanitizeHtml } from "../chat/sanitize";
import type { ChangelogVersion } from "./parseChangelog";

export type { ChangelogSection, ChangelogVersion } from "./parseChangelog";

const ISSUES_URL = "https://github.com/Mumcord/mumcord/issues/";

/** Groups meant for developers; collapsed until opened. */
export const INTERNAL = "Internal";

/** The whole changelog, newest first, embedded at build time. */
export const CHANGELOG_VERSIONS: ChangelogVersion[] = changelog;

/** Builds other than releases show `dev~…` (src-tauri/src/version.rs). */
export function isDevBuild(version: string): boolean {
  return version.startsWith("dev~");
}

/** The numeric parts of "1.2.3", or null for anything else. */
function releaseNumbers(version: string): number[] | null {
  const match = /^(\d+)\.(\d+)\.(\d+)/.exec(version);
  return match === null ? null : [Number(match[1]), Number(match[2]), Number(match[3])];
}

function isNewer(version: string, than: string): boolean {
  const a = releaseNumbers(version);
  const b = releaseNumbers(than);
  if (a === null || b === null) return false;
  for (let i = 0; i < 3; i++) {
    const diff = (a[i] ?? 0) - (b[i] ?? 0);
    if (diff !== 0) return diff > 0;
  }
  return false;
}

/**
 * The versions a build shows, newest first: the running version, then the older releases. An
 * "Unreleased" section is never shown as such. Dev builds show released versions only. A release
 * is built from its tag, where CHANGELOG.md still calls its own section `## [X.Y.Z] - Unreleased`
 * or a plain `## [Unreleased]`; the post-release sync dates it only afterwards. So, like the
 * release notes (release.yml), a release shows that section under its own version number.
 * Versions newer than the running one are hidden.
 */
export function visibleVersions(
  versions: readonly ChangelogVersion[],
  appVersion: string,
): ChangelogVersion[] {
  if (isDevBuild(appVersion)) return versions.filter((v) => !v.unreleased);
  const own =
    versions.find((v) => v.version === appVersion) ??
    versions.find((v) => v.unreleased && v.version === "Unreleased" && v.sections.length > 0);
  return versions.flatMap((v) => {
    if (v !== own) return v.unreleased || isNewer(v.version, appVersion) ? [] : [v];
    if (!v.unreleased) return [v];
    const released: ChangelogVersion = {
      version: appVersion,
      date: v.date,
      unreleased: false,
      sections: v.sections,
    };
    if (v.summary !== undefined) released.summary = v.summary;
    // The link of a plain "[Unreleased]" compares against the previous tag, not this release.
    if (v.url !== undefined && v.version === appVersion) released.url = v.url;
    return [released];
  });
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
