/**
 * The mini bar of a plugin activity (FR-9.49, issue #163): what remains at the top of the chat
 * once the media area is closed or minimised. The user stays in the activity; "Open" brings it
 * back to the stage and the close button leaves it.
 */
import { useTranslation } from "react-i18next";
import { announce } from "../../../a11y";
import type { StageActivityView } from "../../../api";
import { openMedia, useMediaView } from "../../media/mediaView";
import { MiniBar } from "../../media/MiniBar";
import { usePluginIcon } from "../render/usePluginIcon";
import { leaveActivity, useStageActivities } from "./activityStore";
import { StageIcon } from "./StageHeader";

function StageMiniBar({ activity }: { activity: StageActivityView }) {
  const { t } = useTranslation("pluginStage");
  const { t: tc } = useTranslation("common");
  const icon = usePluginIcon(activity.plugin);
  const name = activity.pluginName || activity.plugin;
  const count = activity.participants.length;
  return (
    <MiniBar
      label={t("mediaLabel", { plugin: name })}
      title={activity.title || name}
      thumbnail={<StageIcon id={activity.plugin} name={name} url={icon} />}
      progress={
        activity.startedByMe
          ? t("startedByYou", { count })
          : t("startedBy", { name: activity.starterName, count })
      }
      status={null}
      openLabel={tc("media.open")}
      onOpen={() => {
        openMedia(`plugin:${activity.plugin}`);
      }}
      closeLabel={t("leave", { plugin: name })}
      onClose={() => {
        void leaveActivity(activity.id).then((left) => {
          if (!left) announce(t("leaveFailed", { plugin: name }), "assertive");
        });
      }}
    />
  );
}

/** One mini bar per activity the user is in while the media area is closed. */
export function PluginMiniBars() {
  const view = useMediaView();
  const activities = useStageActivities();
  if (view.cinema !== null) return null;
  return activities
    .filter(
      (activity) =>
        activity.kind === "activity" &&
        activity.joined &&
        view.stage.includes(`plugin:${activity.plugin}`),
    )
    .map((activity) => <StageMiniBar key={activity.id} activity={activity} />);
}
