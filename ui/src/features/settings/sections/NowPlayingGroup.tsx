import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type NowPlayingStatus } from "../../../api";
import { trackLabel } from "../../nowplaying";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import type { SettingsSectionProps } from "./types";

/** How often the detected track is refreshed while the group is visible and sharing is on. */
export const STATUS_REFRESH_MS = 2000;

/**
 * Profile: share the track playing in the system's media player (FR-1.58). Off by default; shows
 * what is currently detected so the user can see what others see.
 */
export function NowPlayingGroup({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const [status, setStatus] = useState<NowPlayingStatus | null>(null);
  const enabled = settings.nowPlaying;

  useEffect(() => {
    let active = true;
    const load = () => {
      getApi()
        .nowPlayingStatus()
        .then(
          (loaded) => {
            if (active) setStatus(loaded);
          },
          (e: unknown) => {
            console.error("Reading the now-playing status failed", e);
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
    <Group title={t("nowPlaying.title")} icon="music">
      <Switch
        label={t("nowPlaying.switch")}
        description={t("nowPlaying.hint")}
        checked={enabled && supported}
        disabled={!supported}
        onChange={(on) => {
          update({ nowPlaying: on });
        }}
      />
      {!supported && <p className="settings-hint">{t("nowPlaying.unsupported")}</p>}
      {supported && enabled && status !== null && (
        <p className="settings-hint" role="status">
          {status.track
            ? t("nowPlaying.current", { track: trackLabel(status.track) })
            : t("nowPlaying.nothing")}
        </p>
      )}
    </Group>
  );
}
