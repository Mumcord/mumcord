/**
 * A plugin's web content in one place (stage, side panel, dialog or panel popover) with the
 * chrome Mumcord draws around it (FR-9.36): the "Plugin content · <plugin>[ · Unofficial]" tag,
 * the mute button once the page plays sound (FR-9.43) and "<plugin> stopped responding" with
 * Reload when its heartbeat stops (FR-9.42). The surrounding container (stage header, side
 * panel head, dialog title and footer) belongs to the place's own host.
 *
 * The page itself is an official or approved plugin page in a frame with an opaque origin
 * (FR-6.46 … FR-6.48): `sandbox="allow-scripts"` without `allow-same-origin` keeps it away from
 * storage, the app window and Tauri; the page's own Content-Security-Policy (set by the
 * `mumcord-web` protocol) limits the network. The frame may load its page once: any further
 * load is a navigation, and the frame is removed.
 *
 * Keyboard (FR-9.41): Tab moves into and out of the frame as usual; Escape or F6 inside the page
 * moves focus to the frame's wrapper (Escape also reaches `onEscape`, e.g. to close a dialog).
 */
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, toEngineError, type PluginWebviewPlace } from "../../../api";
import { useStageShown } from "../../media/stageContext";
import { usePlugins } from "../pluginStore";
import type { PluginWebview } from "../render/pluginUiStore";
import { createBridge, newGestureKey, type Bridge } from "./bridge";
import { setPluginFramesMuted, useFrameMute } from "./frameMute";
import { watchHeartbeat } from "./heartbeat";
import "./webview.css";

interface Props {
  readonly plugin: string;
  readonly webview: PluginWebview;
  /** Escape inside the page; focus has already moved out of the frame. */
  readonly onEscape?: () => void;
}

/** Renders an open web view; the caller decides where (see {@link usePluginWebView}). */
export function PluginFrame({ plugin, webview, onEscape }: Props) {
  const { t } = useTranslation("pluginStage");
  const installed = usePlugins().overview?.plugins.find((p) => p.id === plugin);
  const name = installed?.name || plugin;
  const unofficial = installed !== undefined && installed.source.kind !== "official";
  const { byUser, muted } = useFrameMute(plugin);
  // A hidden stage tab keeps running; its heartbeat is only watched while it shows.
  const shown = useStageShown();

  const [reloads, setReloads] = useState(0);
  const [stopped, setStopped] = useState(false);
  const [sound, setSound] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const reload = useRef<HTMLButtonElement>(null);
  const focusFrame = useRef(false);

  const onStopped = () => {
    // Focus inside the removed frame would fall to the page body; Reload takes it instead.
    focusFrame.current = root.current?.contains(document.activeElement) ?? false;
    setStopped(true);
    setSound(false);
  };

  useEffect(() => {
    if (stopped && focusFrame.current) reload.current?.focus();
  }, [stopped]);

  const onReload = () => {
    setStopped(false);
    setReloads((n) => n + 1);
  };

  const muteLabel = t(byUser ? "frame.unmute" : "frame.mute", { plugin: name });
  return (
    <div ref={root} className="plugin-frame" data-place={webview.place}>
      {stopped ? (
        <div className="plugin-frame__stopped">
          <p role="alert">{t("frame.stopped", { plugin: name })}</p>
          <button ref={reload} type="button" className="button" onClick={onReload}>
            {t("frame.reload")}
          </button>
        </div>
      ) : (
        // A new `seq` (the plugin showed a page again) or a reload gets a fresh frame.
        <SealedFrame
          key={`${String(webview.seq)}:${String(reloads)}`}
          plugin={plugin}
          place={webview.place}
          url={webview.url}
          label={t("frame.label", { plugin: name })}
          muted={muted}
          watched={shown}
          focusOnMount={reloads > 0}
          onEscape={onEscape}
          onSound={() => {
            setSound(true);
          }}
          onStopped={onStopped}
        />
      )}
      <span className="plugin-frame__tag">
        <LockIcon />
        {t(unofficial ? "frame.tagUnofficial" : "frame.tag", { plugin: name })}
      </span>
      {sound && !stopped && (
        <button
          type="button"
          className="plugin-frame__mute"
          aria-label={muteLabel}
          data-tooltip={muteLabel}
          onClick={() => {
            setPluginFramesMuted(plugin, !byUser).catch(() => {
              // Logged in setPluginFramesMuted; the button keeps its state.
            });
          }}
        >
          <SpeakerIcon muted={byUser} />
        </button>
      )}
    </div>
  );
}

interface FrameProps {
  readonly plugin: string;
  readonly place: PluginWebviewPlace;
  readonly url: string;
  readonly label: string;
  readonly muted: boolean;
  /** Whether the heartbeat is watched (the frame is on screen). */
  readonly watched: boolean;
  /** Takes focus when it appears (after Reload). */
  readonly focusOnMount: boolean;
  readonly onEscape: (() => void) | undefined;
  readonly onSound: () => void;
  readonly onStopped: () => void;
}

function SealedFrame(props: FrameProps) {
  const { plugin, place, url, label, muted, watched, focusOnMount } = props;
  const wrapper = useRef<HTMLDivElement>(null);
  const frame = useRef<HTMLIFrameElement>(null);
  const loads = useRef(0);
  const [navigated, setNavigated] = useState(false);
  // One key for the frame's whole life: its bootstrap keeps the first key it gets.
  const [gestureKey] = useState(newGestureKey);
  // The latest callbacks, so the bridge lives as long as the frame.
  const callbacks = useRef(props);
  useEffect(() => {
    callbacks.current = props;
  });

  const bridge = useRef<Bridge | null>(null);
  const watch = useRef<ReturnType<typeof watchHeartbeat> | null>(null);

  useEffect(() => {
    if (focusOnMount) wrapper.current?.focus();
  }, [focusOnMount]);

  useEffect(() => {
    let warned = false;
    const focusApp = () => wrapper.current?.focus();
    const current = createBridge({
      frame: () => frame.current?.contentWindow ?? null,
      toPlugin: (text) => {
        getApi()
          .pluginUiEvent(plugin, { type: "webviewMessage", place, text })
          .catch((error: unknown) => {
            // Dropped by the limits or the plugin stopped; the text is never logged.
            console.debug("A web view message was not delivered", toEngineError(error).code);
          });
      },
      onEscape: () => {
        focusApp();
        callbacks.current.onEscape?.();
      },
      onF6: focusApp,
      onHeartbeat: () => watch.current?.beat(),
      onSound: () => {
        callbacks.current.onSound();
      },
      gestureKey,
      onGesture: () => {
        getApi()
          .pluginUiEvent(plugin, { type: "webviewGesture", place })
          .catch((error: unknown) => {
            // Too fast, or the frame closed meanwhile.
            console.debug("A web view gesture was not counted", toEngineError(error).code);
          });
      },
      onDropped: () => {
        if (warned) return;
        warned = true;
        console.warn("A plugin web view sent messages above the bridge limits; they were dropped");
      },
    });
    bridge.current = current;
    if (loads.current > 0) current.loaded();
    window.addEventListener("message", current.receive);
    const off = getApi().on("pluginUi", (event) => {
      if (
        event.plugin === plugin &&
        event.change.type === "webviewPost" &&
        event.change.places.includes(place)
      ) {
        current.post(event.change.message);
      }
    });
    return () => {
      window.removeEventListener("message", current.receive);
      off();
      bridge.current = null;
    };
  }, [plugin, place, gestureKey]);

  // Sent again after every load, so a fresh page starts silent while muted or deafened.
  useEffect(() => {
    bridge.current?.mute(muted);
  }, [muted]);

  useEffect(() => () => watch.current?.stop(), []);
  useEffect(() => {
    watch.current?.setActive(watched);
  }, [watched]);

  const onLoad = () => {
    loads.current += 1;
    if (loads.current === 1) {
      bridge.current?.loaded();
      // The bootstrap's heartbeat runs from now on.
      watch.current = watchHeartbeat(() => {
        callbacks.current.onStopped();
      });
      watch.current.setActive(watched);
      return;
    }
    // The page navigated away from its own file: never keep what it loaded.
    console.warn("A plugin web view navigated and was closed");
    watch.current?.stop();
    setNavigated(true);
  };

  return (
    <div ref={wrapper} className="plugin-webview" tabIndex={-1}>
      {!navigated && (
        <iframe
          ref={frame}
          className="plugin-webview__frame"
          src={url}
          title={label}
          sandbox="allow-scripts"
          referrerPolicy="no-referrer"
          allow=""
          onLoad={onLoad}
        />
      )}
    </div>
  );
}

function LockIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      width="12"
      height="12"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="4" y="11" width="16" height="10" rx="2" />
      <path d="M8 11V7a4 4 0 0 1 8 0v4" />
    </svg>
  );
}

function SpeakerIcon({ muted }: { muted: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width="16"
      height="16"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M11 5 6 9H2v6h4l5 4z" />
      {muted ? (
        <path d="m22 9-6 6M16 9l6 6" />
      ) : (
        <path d="M15.5 8.5a5 5 0 0 1 0 7M19 5a9 9 0 0 1 0 14" />
      )}
    </svg>
  );
}
