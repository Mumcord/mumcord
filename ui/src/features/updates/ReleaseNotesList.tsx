import { useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ReleaseNote } from "../../api";
import { noteItems, type NoteItem } from "./releaseNotes";

/** The notes of every release since the installed one, newest first and open. */
export function ReleaseNotesList({ notes }: { notes: readonly ReleaseNote[] }) {
  const [open, setOpen] = useState<ReadonlySet<string>>(
    () => new Set(notes[0] === undefined ? [] : [notes[0].version]),
  );
  return (
    <div className="upd-notes">
      {notes.map((note) => (
        <NoteEntry
          key={note.version}
          note={note}
          open={open.has(note.version)}
          onToggle={() => {
            setOpen((old) => {
              const next = new Set(old);
              if (!next.delete(note.version)) next.add(note.version);
              return next;
            });
          }}
        />
      ))}
    </div>
  );
}

function NoteEntry(props: { note: ReleaseNote; open: boolean; onToggle: () => void }) {
  const { t } = useTranslation("appwindow");
  const panelId = useId();
  const { note, open } = props;
  const items = useMemo(() => noteItems(note), [note]);
  return (
    <section className="upd-note">
      <h3 className="upd-note__heading">
        <button
          type="button"
          className="upd-note__toggle"
          aria-expanded={open}
          aria-controls={panelId}
          onClick={props.onToggle}
        >
          <span className="upd-chevron" aria-hidden="true" />
          <span className="upd-note__version">{note.version}</span>
          {note.date !== null && <span className="upd-note__date">{note.date}</span>}
        </button>
      </h3>
      <div id={panelId} hidden={!open}>
        {open &&
          (items.length === 0 ? (
            <p className="upd-note__empty">{t("updater.noNotes")}</p>
          ) : (
            <ul className="upd-note__items">
              {items.map((item, index) => (
                // Entries are plain text that can repeat; their position is their identity.
                <li key={index}>
                  <Tag item={item} />
                  {/* The notes are English only, whatever the UI language is. */}
                  <span lang="en">{item.text}</span>
                </li>
              ))}
            </ul>
          ))}
      </div>
    </section>
  );
}

function Tag({ item }: { item: NoteItem }) {
  const { t } = useTranslation("appwindow");
  switch (item.kind) {
    case "added":
    case "changed":
    case "fixed":
    case "removed":
      return (
        <span className={`upd-tag upd-tag--${item.kind}`}>{t(`updater.tags.${item.kind}`)}</span>
      );
    default:
      return item.title === "" ? null : <span className="upd-tag">{item.title}</span>;
  }
}
