import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ContextMenu, type MenuItem, type MenuPosition } from "../../components/ContextMenu";
import { openSettingsSection } from "../settings/settingsOpener";
import { WhisperKindIcon } from "./WhisperKindIcon";
import { WHISPER_ICON_PATHS } from "./whisperIcons";
import {
  startWhisper,
  stopWhisper,
  useActiveWhisperTarget,
  useWhisperSetup,
  whisperKind,
} from "./whisperStore";
import "./whisper.css";

const GEAR =
  "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z";

function report(action: string) {
  return (error: unknown) => {
    console.error(`${action} failed`, error);
  };
}

/**
 * Chip in the user panel's status strip for whispering from the UI (issue #67), so it works
 * without a key, e.g. where the desktop offers no global shortcuts. Idle, it opens the saved
 * targets; while the user whispers or shouts it says so ("whispering" / "shouting", amber) and
 * stops it.
 */
export function WhisperButton({
  className,
  iconClassName,
}: {
  className: string;
  /** Extra class while idle, when the chip shows only its icon. */
  iconClassName: string;
}) {
  const { t } = useTranslation("whisper");
  const setup = useWhisperSetup();
  const active = useActiveWhisperTarget();
  const [menu, setMenu] = useState<MenuPosition | null>(null);
  const kind = active === null ? null : whisperKind(active.receivers);
  const label =
    active === null || kind === null
      ? t("panel.open")
      : `${t(kind === "whisper" ? "panel.indicatorWhisper" : "panel.indicatorShout", {
          target: active.name,
        })} · ${t("panel.stop")}`;

  const items: MenuItem[] = (setup?.targets ?? []).map((target) => ({
    label: target.name,
    icon: WHISPER_ICON_PATHS[whisperKind(target.receivers)],
    ...(target.hotkey === null ? {} : { hint: target.hotkey }),
    onSelect: () => {
      startWhisper(target.id).catch(report("Starting the whisper"));
    },
  }));
  if (items.length === 0) {
    items.push({ label: t("panel.none"), disabled: true, onSelect: () => {} });
  }
  items.push({
    label: t("panel.manage"),
    icon: GEAR,
    onSelect: () => {
      openSettingsSection("voice");
    },
  });

  return (
    <>
      <button
        type="button"
        className={`${className} whisper-chip ${active === null ? iconClassName : ""}`.trim()}
        aria-label={label}
        data-tooltip={label}
        aria-haspopup={active === null ? "menu" : undefined}
        data-whisper={active === null ? undefined : ""}
        onClick={(event) => {
          if (active !== null) {
            stopWhisper().catch(report("Stopping the whisper"));
            return;
          }
          const box = event.currentTarget.getBoundingClientRect();
          setMenu({ x: box.left, y: box.bottom, opener: event.currentTarget });
        }}
      >
        <WhisperKindIcon kind={kind ?? "whisper"} />
        {kind !== null && (
          <span className="whisper-chip__text" aria-hidden="true">
            {t(kind === "whisper" ? "badge.ownWhisper" : "badge.ownShout")}
          </span>
        )}
      </button>
      {menu !== null && (
        <ContextMenu
          menu={menu}
          label={t("panel.menu")}
          items={items}
          onClose={() => {
            setMenu(null);
          }}
        />
      )}
    </>
  );
}
