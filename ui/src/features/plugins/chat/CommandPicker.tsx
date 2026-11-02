/**
 * The autocomplete list of plugin slash commands (FR-6.43), each with its plugin name. Same
 * combobox pattern as the mention picker: the input keeps the focus and points at the list.
 */
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { commandOptionId, type SlashCommands } from "./useSlashCommands";
import "./pluginChat.css";

export function CommandPicker({ id, slash }: { id: string; slash: SlashCommands }) {
  const { t } = useTranslation("pluginChat");
  const activeRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [slash.active]);
  if (!slash.open) return null;
  return (
    <div className="mention-picker">
      <div id={id} className="mention-picker__list" role="listbox" aria-label={t("command.list")}>
        {slash.options.map((command, index) => (
          // The input keeps the focus and handles the keyboard (aria-activedescendant); the
          // click is the mouse path to the same action.
          // eslint-disable-next-line jsx-a11y/click-events-have-key-events
          <div
            key={`${command.pluginId}:${command.name}`}
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
              slash.pick(command);
            }}
          >
            <span className="mention-picker__keyword">/{command.name}</span>
            <span className="mention-picker__hint">
              {command.description}
              {command.usage === null ? "" : ` · ${command.usage}`}
            </span>
            <span className="plugin-chat-command__plugin">{command.pluginName}</span>
          </div>
        ))}
      </div>
      <p className="mention-picker__keys" aria-hidden="true">
        {t("command.keys")}
      </p>
    </div>
  );
}
