/**
 * The third-party licences (issue #138) as scripts/generate-notices.sh writes them to
 * ui/public/third-party-notices.json: one entry per component, with the licence texts stored
 * once and referenced by id.
 */

export type NoticeSource = "rust" | "npm" | "native";

export interface NoticeEntry {
  name: string;
  /** Empty for system libraries taken from the Linux build host. */
  version: string;
  /** The licence as the component declares it, e.g. "MIT OR Apache-2.0". */
  license: string;
  /** The SPDX identifiers in `license`. */
  licenses: string[];
  repository: string | null;
  source: NoticeSource;
  textIds: string[];
  /** Native components only: what the component is and how Mumcord uses it. */
  description?: string;
}

export interface Notices {
  entries: NoticeEntry[];
  texts: Record<string, string>;
}

/** Stable key of an entry; a crate can appear in several versions. */
export const entryKey = (entry: NoticeEntry) => `${entry.source} ${entry.name} ${entry.version}`;

export async function loadNotices(): Promise<Notices> {
  const response = await fetch(`${import.meta.env.BASE_URL}third-party-notices.json`);
  if (!response.ok) throw new Error(`HTTP ${String(response.status)}`);
  return (await response.json()) as Notices;
}

/** Entries whose name or licence contains `query` (any case) and that use `licence` if set. */
export function filterEntries(
  entries: readonly NoticeEntry[],
  query: string,
  licence: string | null,
): NoticeEntry[] {
  const needle = query.trim().toLowerCase();
  return entries.filter(
    (entry) =>
      (licence === null || entry.licenses.includes(licence)) &&
      (needle === "" ||
        entry.name.toLowerCase().includes(needle) ||
        entry.license.toLowerCase().includes(needle)),
  );
}

/** Every licence id in use, most used first, for the licence filter. */
export function licenceOptions(entries: readonly NoticeEntry[]): { id: string; count: number }[] {
  const counts = new Map<string, number>();
  for (const entry of entries) {
    for (const id of entry.licenses) counts.set(id, (counts.get(id) ?? 0) + 1);
  }
  return [...counts]
    .map(([id, count]) => ({ id, count }))
    .sort((a, b) => b.count - a.count || a.id.localeCompare(b.id));
}

/** The full licence text of an entry; several texts are separated by a blank line. */
export function entryText(entry: NoticeEntry, texts: Readonly<Record<string, string>>): string {
  return entry.textIds.map((id) => texts[id] ?? "").join("\n\n");
}
