/**
 * One plugin activity in the media area (FR-9.46, FR-9.48, FR-9.49, mockup surfaces 03): Mumcord's
 * header, the plugin's content (its `stage` tree or its web page) and the control row with
 * Full screen and Leave. Full screen only ever starts from Mumcord's button, shows the banner for
 * 3 s and Esc always ends it. The plugin hears whether its tab is on screen (`stage-visibility`).
 */
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../../a11y";
import { getApi, toEngineError, type StageActivityView } from "../../../api";
import { useFullscreen } from "../../media/useFullscreen";
import { MediaIcon } from "../../media/icons";
import { useStageShown } from "../../media/stageContext";
import { usePlugins } from "../pluginStore";
import { PluginTree } from "../render/PluginTree";
import { usePluginTree } from "../render/pluginUiStore";
import { usePluginIcon } from "../render/usePluginIcon";
import { PluginFrame, usePluginWebView } from "../webview";
import { leaveActivity } from "./activityStore";
import { StageHeader } from "./StageHeader";
import "../../media/media.css";
import "./stage.css";

/** How long "<plugin> is full screen · Esc to exit" stays (FR-9.49). */
export const FULLSCREEN_BANNER_MS = 3000;

/** Tells the plugin whether its activity is on screen; hidden once the view goes away. */
function useStageVisibility(plugin: string, visible: boolean): void {
  useEffect(() => {
    getApi()
      .stageVisibility(plugin, visible)
      .catch((error: unknown) => {
        console.error("Telling a plugin its stage visibility failed", toEngineError(error).code);
      });
  }, [plugin, visible]);
  useEffect(
    () => () => {
      getApi()
        .stageVisibility(plugin, false)
        .catch((error: unknown) => {
          console.error("Telling a plugin its stage visibility failed", toEngineError(error).code);
        });
    },
    [plugin],
  );
}

/** The plugin's own content: its web page when it has one, otherwise its `stage` tree. */
function StageContent({
  activity,
  onEscape,
}: {
  activity: StageActivityView;
  onEscape: () => void;
}) {
  const webview = usePluginWebView(activity.plugin, "stage");
  const nodes = usePluginTree(activity.plugin, { kind: "stage" });
  if (activity.web)
    return webview ? (
      <PluginFrame plugin={activity.plugin} webview={webview} onEscape={onEscape} />
    ) : null;
  if (nodes === undefined) return null;
  return (
    <div className="plugin-stage__tree">
      <PluginTree plugin={activity.plugin} slot={{ kind: "stage" }} nodes={nodes} />
    </div>
  );
}

export function PluginStage({ activity }: { activity: StageActivityView }) {
  const { t } = useTranslation("pluginStage");
  const { t: tMedia } = useTranslation("common", { keyPrefix: "media" });
  const ref = useRef<HTMLElement>(null);
  const shown = useStageShown();
  const { fullscreen, toggle, exit } = useFullscreen(ref);
  const [banner, setBanner] = useState(false);
  const plugin = usePlugins().overview?.plugins.find((p) => p.id === activity.plugin);
  const icon = usePluginIcon(activity.plugin);
  const name = activity.pluginName || activity.plugin;

  useStageVisibility(activity.plugin, shown);

  // The banner comes with every start of full screen (only ever from the button) for 3 s.
  const bannerTimer = useRef(0);
  useEffect(
    () => () => {
      window.clearTimeout(bannerTimer.current);
    },
    [],
  );
  const onFullscreen = () => {
    if (!fullscreen) {
      setBanner(true);
      window.clearTimeout(bannerTimer.current);
      bannerTimer.current = window.setTimeout(() => {
        setBanner(false);
      }, FULLSCREEN_BANNER_MS);
    }
    toggle();
  };

  // Esc always leaves full screen, whatever has the focus (the plugin's page reports its Esc
  // through the frame's onEscape).
  useEffect(() => {
    if (!fullscreen) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") exit();
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
    };
  }, [fullscreen, exit]);

  const leave = () => {
    if (fullscreen) exit();
    void leaveActivity(activity.id).then((left) => {
      if (!left) announce(t("leaveFailed", { plugin: name }), "assertive");
    });
  };

  const fullscreenLabel = fullscreen ? tMedia("exitFullscreen") : t("fullScreen");
  return (
    <section
      ref={ref}
      className="media-frame plugin-stage"
      aria-label={t("mediaLabel", { plugin: name })}
      data-fullscreen={fullscreen || undefined}
    >
      <StageHeader activity={activity} plugin={plugin} icon={icon} />
      <div className="media-frame__picture plugin-stage__content">
        <StageContent activity={activity} onEscape={exit} />
      </div>
      <div
        className="plugin-stage__bar"
        role="toolbar"
        aria-label={t("controlsLabel", { plugin: name })}
      >
        <button
          type="button"
          className="plugin-stage__button"
          aria-label={fullscreenLabel}
          data-tooltip={fullscreenLabel}
          aria-pressed={fullscreen}
          onClick={onFullscreen}
        >
          <FullscreenIcon exit={fullscreen} />
        </button>
        <button type="button" className="plugin-stage__button" data-tone="leave" onClick={leave}>
          <MediaIcon name="leave" />
          {t("leave", { plugin: name })}
        </button>
      </div>
      {banner && fullscreen && (
        <p className="plugin-stage__banner" role="status">
          {t("fullScreenBanner", { plugin: name })}
        </p>
      )}
    </section>
  );
}

function FullscreenIcon({ exit }: { exit: boolean }) {
  return (
    <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" focusable="false">
      <path
        d={
          exit ? "M6 2v4H2M10 2v4h4M6 14v-4H2M10 14v-4h4" : "M2 6V2h4M14 6V2h-4M2 10v4h4M14 10v4h-4"
        }
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
