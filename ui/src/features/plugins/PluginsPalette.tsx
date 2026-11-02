import { useEffect, useId, useMemo, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { ModalDialog } from "../settings/components/ModalDialog";
import { openSettingsSection } from "../settings/settingsOpener";
import { setAllPluginsOff } from "./pluginStore";

interface Command {
  id: string;
  label: string;
  run: () => void;
}

/** Ctrl+Shift+P (Cmd+Shift+P on macOS) opens the palette, except where an input claims the key. */
const isPaletteShortcut = (event: globalThis.KeyboardEvent) =>
  (event.ctrlKey || event.metaKey) &&
  event.shiftKey &&
  !event.altKey &&
  event.key.toLowerCase() === "p";

/**
 * The command palette (FR-6.6, FR-6.58): a search field over the commands "Plugins" and
 * "Turn off all plugins". Arrow keys move, Enter runs, Escape closes.
 */
export function PluginsPalette() {
  const { t } = useTranslation("pluginSettings");
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.defaultPrevented || !isPaletteShortcut(event)) return;
      event.preventDefault();
      setOpen(true);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, []);

  if (!open) return null;
  return (
    <ModalDialog
      title={t("palette.label")}
      onClose={() => {
        setOpen(false);
      }}
    >
      <PaletteBody
        onDone={() => {
          setOpen(false);
        }}
      />
    </ModalDialog>
  );
}

function PaletteBody({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation("pluginSettings");
  const baseId = useId();
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const commands = useMemo<Command[]>(
    () => [
      {
        id: "open",
        label: t("palette.openPlugins"),
        run: () => {
          openSettingsSection("plugins");
        },
      },
      {
        id: "off",
        label: t("palette.turnOffAll"),
        run: () => {
          void setAllPluginsOff(true);
        },
      },
    ],
    [t],
  );
  const needle = query.trim().toLowerCase();
  const shown = commands.filter((command) => command.label.toLowerCase().includes(needle));
  const current = Math.min(index, shown.length - 1);
  const optionId = (command: Command) => `${baseId}-${command.id}`;

  const run = (command: Command) => {
    onDone();
    command.run();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (shown.length === 0) return;
      const step = event.key === "ArrowDown" ? 1 : -1;
      setIndex((current + step + shown.length) % shown.length);
    } else if (event.key === "Enter") {
      event.preventDefault();
      const command = shown[current];
      if (command) run(command);
    }
  };

  const active = shown[current];
  return (
    <div className="plugins-palette">
      <input
        type="text"
        role="combobox"
        aria-expanded="true"
        aria-controls={`${baseId}-list`}
        aria-activedescendant={active ? optionId(active) : undefined}
        aria-label={t("palette.label")}
        placeholder={t("palette.placeholder")}
        value={query}
        onChange={(event) => {
          setQuery(event.target.value);
          setIndex(0);
        }}
        onKeyDown={onKeyDown}
      />
      <div role="listbox" id={`${baseId}-list`} className="plugins-palette__list">
        {shown.map((command) => (
          // The input keeps focus and handles the keys; the click is for pointer users.
          // eslint-disable-next-line jsx-a11y/click-events-have-key-events
          <div
            key={command.id}
            role="option"
            tabIndex={-1}
            id={optionId(command)}
            aria-selected={command === active}
            className="plugins-palette__item"
            // The input keeps focus and handles the keys; the click is for pointer users.
            onClick={() => {
              run(command);
            }}
          >
            {command.label}
          </div>
        ))}
      </div>
      {shown.length === 0 && <p role="status">{t("palette.empty")}</p>}
    </div>
  );
}
