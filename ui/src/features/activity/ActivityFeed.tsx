/**
 * The activity feed (FR-11.10 – FR-11.14): the events of one server newest first, grouped by day,
 * each as a two-line row. A row whose share, party or activity still exists opens that flow, and a
 * plugin event with an action routes the action back to the plugin; ended rows are plain.
 */
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi } from "../../api";
import { openMedia } from "../media/mediaView";
import { joinActivity, stageActivities, useStageActivities } from "../plugins/stage/activityStore";
import { startWatching, useScreenShares } from "../screenshare";
import { useWatchParty } from "../watchparty";
import { joinWatchParty } from "../watchparty/startParty";
import { useActivityLog, type ActivityEvent } from "./activityStore";
import { ActivityRow } from "./ActivityRow";
import { dayLabel, groupByDay } from "./dayGroups";
import "./activity.css";

export function ActivityFeed({ serverId }: { serverId: number | null }) {
  const { t, i18n } = useTranslation("activity");
  const events = useActivityLog(serverId);
  const shares = useScreenShares(serverId);
  const party = useWatchParty(serverId);
  const activities = useStageActivities();

  if (events.length === 0) return <p className="activity-feed__empty">{t("empty")}</p>;

  // Liveness is checked at render time (FR-11.12), so an ended share, party or activity turns its
  // row back into a plain one without an action.
  const shareSessions = new Set(shares.map((share) => share.session));
  const liveActivities = new Set(
    activities.filter((activity) => activity.kind === "activity").map((activity) => activity.id),
  );
  const liveOf = (event: ActivityEvent): boolean => {
    if (!("live" in event) || !event.started) return false;
    switch (event.live.kind) {
      case "screenShare":
        return shareSessions.has(event.live.session);
      case "watchParty":
        return party !== null;
      case "pluginActivity":
        return liveActivities.has(event.live.id);
    }
  };

  /** What a click on the row opens; nothing for a row that is plain (FR-11.12, FR-11.13). */
  const clickOf = (event: ActivityEvent): (() => void) | undefined => {
    if (event.kind === "pluginEvent") {
      const { pluginId, actionId } = event.plugin;
      if (actionId === null) return undefined;
      return () => {
        getApi()
          .pluginUiAction(pluginId, null, actionId, { type: "none" }, event.serverId)
          .catch((error: unknown) => {
            console.error("A plugin action failed", error);
          });
      };
    }
    if (!("live" in event) || !event.started) return undefined;
    const live = event.live;
    if (live.kind === "screenShare") {
      return () => {
        startWatching(event.serverId, live.session);
      };
    }
    if (live.kind === "watchParty") {
      return () => {
        joinWatchParty(event.serverId, t("joinPartyFailed"));
      };
    }
    const id = live.id;
    return () => {
      const activity = stageActivities().find((entry) => entry.id === id);
      // The activity can have ended between the render and the click; then there is nothing.
      if (activity === undefined) return;
      if (activity.joined) {
        openMedia(`plugin:${activity.plugin}`);
        return;
      }
      void joinActivity(id).then((reason) => {
        if (reason !== null) announce(t("joinActivityFailed"), "assertive");
      });
    };
  };

  const time = new Intl.DateTimeFormat(i18n.language, { timeStyle: "short" });
  const now = new Date();

  return (
    <div className="activity-feed">
      {groupByDay(events).map((group) => (
        <div className="activity-group" key={group.day}>
          <h3 className="activity-group__day">{dayLabel(group.day, now, i18n.language, t)}</h3>
          <ul className="activity-group__list">
            {group.events.map((event, index) => {
              const live = liveOf(event);
              // A plugin event is clickable through its own action even though nothing of it is
              // still running; the other kinds are clickable only while their target exists.
              const run = live || event.kind === "pluginEvent" ? clickOf(event) : undefined;
              return (
                <ActivityRow
                  key={index}
                  event={event}
                  time={time.format(event.at)}
                  live={live}
                  onActivate={run}
                />
              );
            })}
          </ul>
        </div>
      ))}
    </div>
  );
}
