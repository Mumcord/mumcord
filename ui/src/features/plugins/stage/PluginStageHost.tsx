/**
 * Puts every activity the user is in into the media area as a stage tab (FR-9.44, FR-9.46),
 * through the stage registry: the tab comes when the user started or joined it and goes when
 * they leave it or it ends. Also shows the host's refusals of a plugin's own `stage.start` or
 * `stage.join` ("Too many activities in this channel", "This activity is full").
 */
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../../a11y";
import { getApi, type StageActivityView } from "../../../api";
import { registerStageItem } from "../../media/stageRegistry";
import { showPluginToast } from "../prompt/installToast";
import { usePluginIcon } from "../render/usePluginIcon";
import { useStageActivities } from "./activityStore";
import { PluginStage } from "./PluginStage";
import { StageIcon } from "./StageHeader";

function StageEntry({ activity }: { activity: StageActivityView }) {
  const icon = usePluginIcon(activity.plugin);
  const unregister = useRef<(() => void) | null>(null);
  const label = activity.pluginName || activity.plugin;
  // Registering the same id again only updates the tab, so a change never closes it.
  useEffect(() => {
    unregister.current = registerStageItem(`plugin:${activity.plugin}`, {
      label,
      icon: <StageIcon name={label} url={icon} small />,
      render: () => <PluginStage activity={activity} />,
    });
  }, [activity, label, icon]);
  useEffect(
    () => () => {
      unregister.current?.();
      unregister.current = null;
    },
    [activity.plugin],
  );
  return null;
}

/** Shows the limit a plugin's own start or join ran into, spoken and as a toast. */
function useStageRefusals(): void {
  const { t } = useTranslation("pluginStage");
  useEffect(
    () =>
      getApi().on("pluginsStageRefused", ({ reason }) => {
        const text = t(reason === "tooMany" ? "tooMany" : "full");
        showPluginToast(text);
        announce(text, "assertive");
      }),
    [t],
  );
}

/** Mount once, in the main window. */
export function PluginStageHost() {
  useStageRefusals();
  // At most one activity per plugin per channel (FR-9.46), so the plugin id names the tab.
  return useStageActivities()
    .filter((activity) => activity.kind === "activity" && activity.joined)
    .map((activity) => <StageEntry key={activity.plugin} activity={activity} />);
}
