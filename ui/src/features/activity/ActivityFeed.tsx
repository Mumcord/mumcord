/**
 * The activity feed (FR-11.10 – FR-11.14): the events of one server newest first, grouped by day,
 * each as a two-line row. A row whose share, party or activity still exists opens that flow, and a
 * plugin event with an action routes the action back to the plugin; ended rows are plain. When the
 * log also holds events from before this connection (#594), an "Earlier" divider marks where this
 * session starts.
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
import { dayLabel, groupByDay, splitAtEarlier } from "./dayGroups";
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
    // Events from before this connection point at sessions and ids that may be reused by now.
    if (!("live" in event) || !event.started || event.earlier === true) return false;
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
    // Your own share, party or activity: you are already in it, so there is nothing to watch or join.
    if (event.self) return undefined;
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

  const list = (part: readonly ActivityEvent[]) => (
    <ul className="activity-group__list">
      {part.map((event, index) => {
        const live = liveOf(event);
        // A plugin event of this connection is clickable through its own action even though
        // nothing of it is still running; one from before it may name an action the plugin no
        // longer knows. The other kinds are clickable only while their target exists.
        const pluginAction = event.kind === "pluginEvent" && event.earlier !== true;
        const run = live || pluginAction ? clickOf(event) : undefined;
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
  );
  const divider = (
    <div className="activity-feed__earlier" role="separator" aria-label={t("earlierLabel")}>
      {t("earlier")}
    </div>
  );

  return (
    <div className="activity-feed">
      {splitAtEarlier(groupByDay(events)).map(({ group, before, current, earlier }) => (
        <div className="activity-group" key={group.day}>
          {before && divider}
          <h3 className="activity-group__day">{dayLabel(group.day, now, i18n.language, t)}</h3>
          {current.length > 0 && list(current)}
          {current.length > 0 && earlier.length > 0 && divider}
          {earlier.length > 0 && list(earlier)}
        </div>
      ))}
    </div>
  );
}
