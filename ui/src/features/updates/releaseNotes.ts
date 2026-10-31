import type { ReleaseNote } from "../../api";
import { groupKind, INTERNAL, plainText, type GroupKind } from "../changelog/changelog";
import { parseChangelog } from "../changelog/parseChangelog";

export interface NoteItem {
  kind: GroupKind;
  /** The group heading as written, for groups without a known tag. */
  title: string;
  /** Plain text: the notes come from the network, so their Markdown is never rendered as HTML. */
  text: string;
}

/**
 * The entries of one release's notes (`### Added` …), in order. Internal changes are left out:
 * they mean nothing to someone deciding whether to update.
 */
export function noteItems(note: ReleaseNote): NoteItem[] {
  // The notes are one CHANGELOG section without its version heading; the parser needs one.
  const [version] = parseChangelog(`## [${note.version}]\n${note.changes}`);
  if (version === undefined) return [];
  return version.sections
    .filter((section) => section.title !== INTERNAL)
    .flatMap((section) =>
      section.items.map((item) => ({
        kind: groupKind(section.title),
        title: section.title,
        text: plainText(item),
      })),
    );
}
