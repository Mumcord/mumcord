/**
 * The activity lines at the top of the chat (FR-9.47): "<name> started <plugin> · Join" for
 * activities of a plugin the user has, and, for official plugins only, "<name> is playing
 * <plugin> · Get plugin" for people without it, which opens the normal install prompt. Join is
 * the user's click: the host joins and the activity's tab appears; when that fails the line says
 * why ("This activity is full" …).
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../../a11y";
import type { StageActivityView, StageJoinError } from "../../../api";
import { openMedia } from "../../media/mediaView";
import { routePluginRequest } from "../pluginRequestStore";
import { usePluginIcon } from "../render/usePluginIcon";
import { joinActivity, useStageActivities } from "../stage/activityStore";
import { StageIcon } from "../stage/StageHeader";
import "./ActivityLine.css";

const ERROR_KEY = {
  gone: "joinGone",
  full: "full",
  notPermitted: "joinNotPermitted",
  failed: "joinFailed",
} as const satisfies Record<StageJoinError, string>;

export function ActivityLine({ activity }: { activity: StageActivityView }) {
  const { t } = useTranslation("pluginStage");
  const icon = usePluginIcon(activity.kind === "activity" ? activity.plugin : null);
  const [error, setError] = useState<StageJoinError | null>(null);
  const [busy, setBusy] = useState(false);
  const plugin = activity.pluginName || activity.plugin;
  const getPlugin = activity.kind === "getPlugin";

  const join = () => {
    if (activity.joined) {
      openMedia(`plugin:${activity.plugin}`);
      return;
    }
    setBusy(true);
    setError(null);
    void joinActivity(activity.id).then((reason) => {
      setBusy(false);
      setError(reason);
      if (reason !== null) announce(t(ERROR_KEY[reason], { plugin }), "assertive");
    });
  };

  return (
    <li className="plugin-activity">
      <StageIcon id={activity.plugin} name={plugin} url={icon} small />
      <span className="plugin-activity__text">
        {t(getPlugin ? "activityPlaying" : "activityStarted", {
          name: activity.starterName,
          plugin,
        })}
      </span>
      <span className="plugin-activity__dot" aria-hidden="true">
        ·
      </span>
      {getPlugin ? (
        <button
          type="button"
          className="plugin-activity__action"
          aria-label={t("getPluginLabel", { plugin })}
          onClick={() => {
            routePluginRequest({ kind: "install", plugin: activity.plugin });
          }}
        >
          {t("getPlugin")}
        </button>
      ) : (
        <button
          type="button"
          className="plugin-activity__action"
          aria-label={t(activity.joined ? "openLabel" : "joinLabel", { plugin })}
          aria-disabled={busy || undefined}
          onClick={busy ? undefined : join}
        >
          {t(activity.joined ? "open" : "join")}
        </button>
      )}
      {error !== null && (
        <span className="plugin-activity__error">{t(ERROR_KEY[error], { plugin })}</span>
      )}
    </li>
  );
}

/** The lines for the user's channel; the user's own activities need none. */
export function PluginActivityLines() {
  const { t } = useTranslation("pluginStage");
  const lines = useStageActivities().filter((activity) => !activity.startedByMe);
  if (lines.length === 0) return null;
  return (
    <ul className="plugin-activities" aria-label={t("activitiesLabel")}>
      {lines.map((activity) => (
        <ActivityLine key={activity.id} activity={activity} />
      ))}
    </ul>
  );
}
