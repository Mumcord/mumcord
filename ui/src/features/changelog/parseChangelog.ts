/**
 * Turns CHANGELOG.md into the data of the "What's new" window (issue #145). Runs at build time
 * (see `changelogJson` in vite.config.ts), so it uses no browser or Node APIs.
 */

export interface ChangelogSection {
  /** The `###` heading, e.g. "Added"; empty for items written before any heading. */
  title: string;
  /** One Markdown string per list item. */
  items: string[];
}

export interface ChangelogVersion {
  /** "0.4.4", or "Unreleased" for a `## [Unreleased]` section. */
  version: string;
  /** `YYYY-MM-DD`; `null` while unreleased. */
  date: string | null;
  unreleased: boolean;
  /** The paragraph between the version heading and its first group, as Markdown. */
  summary?: string;
  /** The release page from the link definitions at the end of the file. */
  url?: string;
  sections: ChangelogSection[];
}

// `## [0.4.3] - 2026-10-28`, `## [0.4.4] - Unreleased`, `## [Unreleased]`; the brackets and
// the kind of dash are optional.
const VERSION_HEADING = /^##\s+\[?([^\]\s]+)\]?(?:\s+[-–—]\s+(.*?))?\s*$/;
const SECTION_HEADING = /^###\s+(.+?)\s*#*\s*$/;
const ITEM = /^[-*+]\s+(.*)$/;
const DATE = /^\d{4}-\d{2}-\d{2}$/;
const LINK_DEFINITION = /^\[([^\]]+)\]:\s+(\S+)\s*$/;

export function parseChangelog(markdown: string): ChangelogVersion[] {
  const versions: ChangelogVersion[] = [];
  const links = new Map<string, string>();
  let version: ChangelogVersion | null = null;
  let section: ChangelogSection | null = null;
  let summary: string[] = [];
  // The item continuation lines belong to; `null` after a blank line or a heading.
  let open: { section: ChangelogSection; index: number } | null = null;

  const closeSummary = () => {
    if (version !== null && summary.length > 0 && version.summary === undefined) {
      version.summary = summary.join(" ");
    }
    summary = [];
  };

  for (const raw of markdown.split(/\r?\n/)) {
    const line = raw.trimEnd();
    const definition = LINK_DEFINITION.exec(line);
    if (definition?.[1] !== undefined && definition[2] !== undefined) {
      links.set(definition[1].toLowerCase(), definition[2]);
      open = null;
      continue;
    }
    if (line.startsWith("## ")) {
      closeSummary();
      section = null;
      open = null;
      const heading = VERSION_HEADING.exec(line);
      version = heading?.[1] === undefined ? null : newVersion(heading[1], heading[2]);
      if (version !== null) versions.push(version);
      continue;
    }
    if (version === null) continue;
    if (line.startsWith("### ")) {
      closeSummary();
      open = null;
      const title = SECTION_HEADING.exec(line)?.[1] ?? "";
      section = { title, items: [] };
      version.sections.push(section);
      continue;
    }
    if (line.trim() === "") {
      closeSummary();
      open = null;
      continue;
    }
    const item = ITEM.exec(line);
    if (item?.[1] !== undefined && item[1].trim() !== "") {
      closeSummary();
      if (section === null) {
        section = { title: "", items: [] };
        version.sections.push(section);
      }
      section.items.push(item[1].trim());
      open = { section, index: section.items.length - 1 };
      continue;
    }
    if (open !== null && /^\s/.test(line)) {
      // An indented line continues the item above, e.g. a wrapped line or a nested list.
      const text = open.section.items[open.index] ?? "";
      open.section.items[open.index] = `${text}\n${line.replace(/^ {1,4}/, "")}`;
      continue;
    }
    if (section === null) {
      summary.push(line.trim());
    } else {
      // A paragraph inside a group reads as one more entry rather than being lost.
      section.items.push(line.trim());
      open = { section, index: section.items.length - 1 };
    }
  }
  closeSummary();

  for (const entry of versions) {
    entry.sections = entry.sections.filter((s) => s.items.length > 0);
    const url = links.get(entry.version.toLowerCase());
    if (url !== undefined) entry.url = url;
  }
  return versions;
}

function newVersion(name: string, suffix: string | undefined): ChangelogVersion {
  const label = (suffix ?? "").trim();
  const unreleased = /^unreleased$/i.test(name) || /unreleased/i.test(label);
  const date = DATE.test(label) ? label : null;
  return {
    version: /^unreleased$/i.test(name) ? "Unreleased" : name,
    date: unreleased ? null : date,
    unreleased,
    sections: [],
  };
}
