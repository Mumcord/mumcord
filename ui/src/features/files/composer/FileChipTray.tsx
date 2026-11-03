/** The files waiting to be shared (mockup 1b): icon, name, size or progress, and a remove button. */
import { useTranslation } from "react-i18next";
import type { FileChip } from "../../../bindings/commands";
import { chipStatus } from "./chipView";
import "./composer.css";

const FILE_PATH =
  "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8zM14 2v6h6M8 13h8M8 17h5";
const ZIP_PATH =
  "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8zM14 2v6h6M10 4h1M10 7h1M10 10h1M10 13h1v3h-1z";

function Svg({ d, size }: { d: string; size: number }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      aria-hidden="true"
      focusable="false"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d={d} />
    </svg>
  );
}

export function FileChipTray({
  chips,
  onRemove,
}: {
  chips: readonly FileChip[];
  onRemove: (chipId: number) => void;
}) {
  const { t, i18n } = useTranslation("files");
  if (chips.length === 0) return null;
  return (
    <ul className="chat-tray" aria-label={t("composer.trayLabel")}>
      {chips.map((chip) => {
        const kind = chip.state.kind;
        const failed = kind === "failed";
        const removeLabel =
          kind === "zipping"
            ? t("composer.stopZipping", { name: chip.name })
            : t("composer.remove", { name: chip.name });
        return (
          <li
            key={chip.id}
            className="chat-tray__item file-chip"
            data-state={kind}
            aria-busy={kind === "zipping" || kind === "checking"}
          >
            <span className="file-chip__icon">
              <Svg d={chip.name.toLowerCase().endsWith(".zip") ? ZIP_PATH : FILE_PATH} size={22} />
            </span>
            <span className="chat-tray__info">
              <span className="chat-tray__name">{chip.name}</span>
              <span
                className={failed ? "file-chip__error" : "chat-tray__note"}
                role={failed ? "alert" : undefined}
              >
                {chipStatus(t, i18n.language, chip)}
              </span>
            </span>
            <button
              type="button"
              className="chat-tray__remove"
              aria-label={removeLabel}
              data-tooltip={removeLabel}
              onClick={() => {
                onRemove(chip.id);
              }}
            >
              <Svg d="M6 6l12 12M18 6L6 18" size={14} />
            </button>
          </li>
        );
      })}
    </ul>
  );
}
