/**
 * The command list above the message input (Epic 4, issues #378 and FR-6.43): Mumcord's admin
 * commands first, then the plugins', and while an argument is being typed the people or channels
 * it may name. Same combobox pattern as the mention picker: the input keeps the focus and points
 * at the list with `aria-activedescendant`.
 */
import { useEffect, useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { commandOptionId, type SlashBox, type SlashOption } from "./slashCommands";

interface Indexed {
  option: SlashOption;
  index: number;
}

export function SlashPicker({ id, slash }: { id: string; slash: SlashBox }) {
  const { t } = useTranslation(["chat", "pluginChat"]);
  const activeRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [slash.active]);
  if (!slash.open) return null;

  const rows: Indexed[] = slash.options.map((option, index) => ({ option, index }));
  // Rows of one section follow each other, so a section starts wherever its group changes.
  const sections: { group: string; rows: Indexed[] }[] = [];
  for (const row of rows) {
    const last = sections[sections.length - 1];
    if (last !== undefined && last.group === row.option.group) last.rows.push(row);
    else sections.push({ group: row.option.group, rows: [row] });
  }

  function row({ option, index }: Indexed): ReactNode {
    return (
      // The input keeps the focus and handles the keyboard (aria-activedescendant); the click is
      // the mouse path to the same action.
      // eslint-disable-next-line jsx-a11y/click-events-have-key-events
      <div
        key={option.id}
        ref={index === slash.active ? activeRef : undefined}
        id={commandOptionId(id, index)}
        className="mention-picker__option"
        role="option"
        tabIndex={-1}
        aria-selected={index === slash.active}
        onMouseDown={(event) => {
          event.preventDefault();
        }}
        onClick={() => {
          slash.pick(option);
        }}
      >
        <span
          className={
            option.keyword.startsWith("/") ? "mention-picker__keyword" : "mention-picker__name"
          }
        >
          {option.keyword}
        </span>
        {option.hint === "" ? null : <span className="mention-picker__hint">{option.hint}</span>}
        {option.tag === "" ? null : <span className="slash-picker__tag">{option.tag}</span>}
      </div>
    );
  }

  return (
    <div className="mention-picker">
      <div
        id={id}
        className="mention-picker__list"
        role="listbox"
        aria-label={t("chat:slash.list")}
      >
        {sections.map((section, position) => {
          const headingId = `${id}-section-${String(position)}`;
          return (
            <div key={section.group} role="group" aria-labelledby={headingId}>
              <div id={headingId} className="mention-picker__section" role="presentation">
                {section.group}
              </div>
              {section.rows.map(row)}
            </div>
          );
        })}
      </div>
      <p className="mention-picker__keys" aria-hidden="true">
        {t("pluginChat:command.keys")}
      </p>
    </div>
  );
}
