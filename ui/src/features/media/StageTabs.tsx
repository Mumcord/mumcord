/**
 * The stage switcher of the media area (FR-9.44, FR-9.50, mockup surfaces 03): one tab per item
 * once more than one item runs, Alt+1 … Alt+9 and the arrow keys switch, and a tab that came while
 * another item was shown carries a dot until it is picked.
 */
import { useEffect, useRef, type KeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { RoomIcon } from "../camera/room/icons";
import { MonitorIcon } from "../screenshare/icons";
import { MediaIcon } from "./icons";
import { selectStage, useMediaView, type StageId } from "./mediaView";
import { stageDomId } from "./stageContext";
import { useStageEntries, type StageEntries } from "./stageRegistry";

const MAX_SHORTCUT = 9;

interface Tab {
  id: StageId;
  label: string;
  icon: ReactNode;
  plugin: boolean;
}

function useTabs(stage: readonly StageId[], entries: StageEntries): Tab[] {
  const { t: tShare } = useTranslation("common", { keyPrefix: "media.stage" });
  const { t: tWatch } = useTranslation("watchparty");
  const { t: tCamera } = useTranslation("camera");
  return stage.map((id): Tab => {
    if (id === "camera")
      return {
        id,
        label: entries.labels.camera ?? tCamera("room.title"),
        icon: <RoomIcon name="camera" size={16} />,
        plugin: false,
      };
    if (id === "share")
      return {
        id,
        label: entries.labels.share ?? tShare("share"),
        icon: <MonitorIcon />,
        plugin: false,
      };
    if (id === "watch")
      return {
        id,
        label: entries.labels.watch ?? tWatch("title"),
        icon: <MediaIcon name="play" />,
        plugin: false,
      };
    const item = entries.plugins.get(id);
    return { id, label: item?.label ?? "", icon: item?.icon ?? null, plugin: true };
  });
}

/** "Alt+1 / Alt+2" for two tabs, "Alt+1 … Alt+5" for more. */
function shortcutText(count: number): string {
  const last = Math.min(count, MAX_SHORTCUT);
  return last === 2 ? "Alt+1 / Alt+2" : `Alt+1 … Alt+${String(last)}`;
}

export function StageTabs() {
  const { t } = useTranslation("common", { keyPrefix: "media.stage" });
  const view = useMediaView();
  const tabs = useTabs(view.stage, useStageEntries());
  const row = useRef<HTMLDivElement>(null);
  const shown = tabs.length > 1;

  // Alt+1 … Alt+9 work wherever focus is while the row is there (FR-9.50). The physical key
  // counts, since Alt with a digit types another character on some layouts.
  const stage = view.stage;
  useEffect(() => {
    if (!shown) return;
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      const digit = /^Digit([1-9])$/.exec(event.code);
      if (digit === null) return;
      const id = stage[Number(digit[1]) - 1];
      if (id === undefined) return;
      event.preventDefault();
      selectStage(id);
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [shown, stage]);

  if (!shown) return null;

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    const at = tabs.findIndex((tab) => tab.id === view.cinema);
    const last = tabs.length - 1;
    const next =
      event.key === "ArrowRight"
        ? at === last
          ? 0
          : at + 1
        : event.key === "ArrowLeft"
          ? at <= 0
            ? last
            : at - 1
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? last
              : null;
    if (next === null) return;
    event.preventDefault();
    const tab = tabs[next];
    if (tab === undefined) return;
    selectStage(tab.id);
    row.current?.querySelector<HTMLElement>(`#${stageDomId("tab", tab.id)}`)?.focus();
  };

  return (
    <div className="media-stage__row">
      <div
        ref={row}
        className="media-panel__tabs media-stage__tabs"
        role="tablist"
        aria-label={t("tabs")}
      >
        {tabs.map((tab, index) => {
          const selected = tab.id === view.cinema;
          const unseen = view.unseen.includes(tab.id);
          const name = t(
            `${tab.plugin ? "pluginTabName" : "tabName"}${unseen ? "New" : ""}` as const,
            { label: tab.label, index: index + 1, count: tabs.length },
          );
          return (
            <button
              key={tab.id}
              id={stageDomId("tab", tab.id)}
              type="button"
              role="tab"
              className="media-panel__tab media-stage__tab"
              aria-selected={selected}
              aria-controls={stageDomId("panel", tab.id)}
              aria-label={name}
              aria-keyshortcuts={index < MAX_SHORTCUT ? `Alt+${String(index + 1)}` : undefined}
              tabIndex={selected ? 0 : -1}
              onClick={() => {
                selectStage(tab.id);
              }}
              onKeyDown={onKeyDown}
            >
              {tab.icon !== null && (
                <span className="media-stage__icon" aria-hidden="true">
                  {tab.icon}
                </span>
              )}
              <span className="media-stage__label">{tab.label}</span>
              {tab.plugin && (
                <span className="media-stage__chip" aria-hidden="true">
                  {t("plugin")}
                </span>
              )}
              {unseen && <span className="media-stage__dot" aria-hidden="true" />}
            </button>
          );
        })}
      </div>
      <span className="media-stage__hint">
        {t("hint")} · {shortcutText(tabs.length)}
      </span>
    </div>
  );
}
