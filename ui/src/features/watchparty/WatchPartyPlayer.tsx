/**
 * The party's YouTube player. It runs in the app's loopback player page (FR-2.41a), because
 * YouTube refuses to play inside the app's own `tauri://` origin (error 153); the same page is
 * used on every platform. When the page or the player reports an error or does not answer within
 * {@link READY_TIMEOUT_MS}, a hint offers the video in the browser instead, with the install hint
 * when the webview lacks H.264 (FR-2.41a).
 */
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type WatchCommand } from "../../api";
import { openInBrowser } from "../chat/openLink";
import { CodecHint } from "./CodecHint";
import { PartySync } from "./partySync";
import { ducker } from "./ducking";
import {
  loadCommand,
  parseBridgeReport,
  playerOrigin,
  volumeCommand,
  type BridgeCommand,
} from "./playerBridge";
import type { LiveParty } from "./watchPartyStore";
import { ENDED, watchUrl } from "./youtube";

export const READY_TIMEOUT_MS = 8000;

interface WatchPartyPlayerProps {
  live: LiveParty;
  onCommand: (command: WatchCommand) => void;
  /** The video of playlist entry `entry` ended. */
  onEnded: (entry: number) => void;
}

export function WatchPartyPlayer({ live, onCommand, onEnded }: WatchPartyPlayerProps) {
  const { t } = useTranslation("watchparty");
  const frame = useRef<HTMLIFrameElement>(null);
  const [pageUrl, setPageUrl] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [failed, setFailed] = useState(false);
  const [youTubeError, setYouTubeError] = useState<number | null>(null);
  const onCommandRef = useRef(onCommand);
  const onEndedRef = useRef(onEnded);
  const liveRef = useRef(live);
  useEffect(() => {
    onCommandRef.current = onCommand;
    onEndedRef.current = onEnded;
  }, [onCommand, onEnded]);
  const sync = useRef<PartySync | null>(null);
  const videoId = live.party.videoId;
  const load = loadCommand(videoId);
  const origin = pageUrl === null ? null : playerOrigin(pageUrl);

  useEffect(() => {
    let current = true;
    getApi()
      .watchPartyPlayerUrl()
      .then((url) => {
        if (!current) return;
        if (playerOrigin(url) === null) {
          console.warn("Unexpected watch party player URL");
          setFailed(true);
        } else {
          setPageUrl(url);
        }
      })
      .catch((error: unknown) => {
        console.warn("Watch party player page unavailable", error);
        if (current) setFailed(true);
      });
    return () => {
      current = false;
    };
  }, []);

  useEffect(() => {
    const post = (command: BridgeCommand | null) => {
      if (origin !== null && command !== null) {
        frame.current?.contentWindow?.postMessage(command, origin);
      }
    };
    const control = new PartySync({
      now: () => Date.now(),
      player: {
        play: () => {
          post({ type: "play" });
        },
        pause: () => {
          post({ type: "pause" });
        },
        seekTo: (positionMs) => {
          post({ type: "seek", seconds: positionMs / 1000 });
        },
      },
      send: (command) => {
        onCommandRef.current(command);
      },
      ended: (entry) => {
        onEndedRef.current(entry);
      },
    });
    sync.current = control;
    // A new sync once the page URL arrives; it starts from the party as it is now.
    control.update(liveRef.current);
    let answered = false;
    const onMessage = (event: MessageEvent) => {
      if (event.origin !== origin || event.source !== frame.current?.contentWindow) return;
      const report = parseBridgeReport(event.data);
      if (report === null) return;
      if (report.type === "error") {
        console.warn(`YouTube player error ${String(report.code)}`);
        answered = true;
        setYouTubeError(report.code);
        setFailed(true);
        return;
      }
      if (!answered) {
        answered = true;
        setReady(true);
        control.onReady();
        // The page starts at the viewer's own volume; only a lowered level needs sending.
        if (ducker.level !== 1) post(volumeCommand(ducker.level));
      }
      if (report.type === "state") control.onState(report.state);
      else if (report.type === "ended") control.onState(ENDED);
      else if (report.type === "time") control.onTime(report.seconds);
    };
    window.addEventListener("message", onMessage);
    // Ducking while people talk (FR-2.76); the page fades and keeps the viewer's own volume.
    const unsubscribeDucking = ducker.subscribe((level) => {
      if (answered) post(volumeCommand(level));
    });
    const timeout = window.setTimeout(() => {
      if (!answered) setFailed(true);
    }, READY_TIMEOUT_MS);
    return () => {
      window.removeEventListener("message", onMessage);
      unsubscribeDucking();
      window.clearTimeout(timeout);
      sync.current = null;
    };
  }, [videoId, origin]);

  useEffect(() => {
    liveRef.current = live;
    sync.current?.update(live);
  }, [live]);

  if (load === null) return null;
  return (
    <div className="watch-party__player">
      {!failed && pageUrl !== null && origin !== null && (
        <iframe
          // A new page per video, as with YouTube's own embed URL before.
          key={videoId}
          ref={frame}
          className="watch-party__frame"
          src={pageUrl}
          title={t("player")}
          allow="autoplay; encrypted-media; fullscreen; picture-in-picture"
          allowFullScreen
          referrerPolicy="no-referrer"
          sandbox="allow-scripts allow-same-origin allow-presentation"
          data-ready={ready}
          onLoad={() => {
            frame.current?.contentWindow?.postMessage(load, origin);
          }}
        />
      )}
      {failed && (
        <div className="watch-party__unavailable" role="status">
          <p>{t("cantPlay")}</p>
          <CodecHint
            failure={youTubeError === null ? null : { kind: "youtube", code: youTubeError }}
            className="watch-party__codec-hint"
          />
          <button
            type="button"
            className="button"
            onClick={() => {
              openInBrowser(watchUrl(videoId));
            }}
          >
            {t("openInBrowser")}
          </button>
        </div>
      )}
    </div>
  );
}
