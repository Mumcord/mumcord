import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type NowGamingStatus } from "../../../api";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import type { SettingsSectionProps } from "./types";

/** How often the detected game is refreshed while the group is visible and sharing is on. */
export const STATUS_REFRESH_MS = 5000;

/**
 * Profile: share the game being played (FR-1.59). Off by default; shows what is currently
 * detected so the user can see what others see.
 */
export function NowGamingGroup({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const [status, setStatus] = useState<NowGamingStatus | null>(null);
  const enabled = settings.nowGaming;

  useEffect(() => {
    let active = true;
    const load = () => {
      getApi()
        .nowGamingStatus()
        .then(
          (loaded) => {
            if (active) setStatus(loaded);
          },
          (e: unknown) => {
            console.error("Reading the now-gaming status failed", e);
          },
        );
    };
    load();
    if (!enabled) {
      return () => {
        active = false;
      };
    }
    const timer = setInterval(load, STATUS_REFRESH_MS);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [enabled]);

  const supported = status?.supported ?? true;
  return (
    <Group title={t("nowGaming.title")}>
      <Switch
        label={t("nowGaming.switch")}
        description={t("nowGaming.hint")}
        checked={enabled && supported}
        disabled={!supported}
        onChange={(on) => {
          update({ nowGaming: on });
        }}
      />
      {!supported && <p className="settings-hint">{t("nowGaming.unsupported")}</p>}
      {supported && enabled && status !== null && (
        <p className="settings-hint" role="status">
          {status.game
            ? t("nowGaming.current", { game: status.game.name })
            : t("nowGaming.nothing")}
        </p>
      )}
    </Group>
  );
}
