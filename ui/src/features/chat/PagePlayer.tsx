/**
 * A chat card's YouTube or Twitch player, run inside the app's loopback player page (FR-2.41a):
 * YouTube refuses to play in the app's own `tauri://` origin and Twitch needs an https or
 * `localhost` parent. The page is opened when the player mounts and closed when it unmounts, so
 * the app stops its server once nothing plays. A player that fails for lack of H.264 gets the
 * install hint under it (FR-2.41a).
 */
import { useEffect, useRef, useState } from "react";
import { getApi } from "../../api";
import {
  CodecHint,
  isTwitchPlayer,
  parseBridgeReport,
  playerOrigin,
  type BridgeCommand,
  type PlaybackFailure,
} from "../watchparty";

interface PagePlayerProps {
  /** The engine's player URL; the page checks it again and adds Twitch's `parent`. */
  playerUrl: string;
  title: string;
  /** The page could not be opened. */
  onFailed: () => void;
}

export function PagePlayer({ playerUrl, title, onFailed }: PagePlayerProps) {
  const frame = useRef<HTMLIFrameElement>(null);
  const [pageUrl, setPageUrl] = useState<string | null>(null);
  const [youTubeError, setYouTubeError] = useState<number | null>(null);
  const onFailedRef = useRef(onFailed);
  useEffect(() => {
    onFailedRef.current = onFailed;
  }, [onFailed]);

  useEffect(() => {
    let current = true;
    const opened = getApi().embedPlayerOpen();
    opened
      .then((url) => {
        if (!current) return;
        if (playerOrigin(url, "embed") === null) {
          console.warn("Unexpected embed player URL");
          onFailedRef.current();
        } else {
          setPageUrl(url);
        }
      })
      .catch((error: unknown) => {
        console.warn("Embed player page unavailable", error);
        if (current) onFailedRef.current();
      });
    return () => {
      current = false;
      // Only a page that was opened is closed, also when the card goes away before it was.
      opened.then(
        () =>
          getApi()
            .embedPlayerClose()
            .catch((error: unknown) => {
              console.warn("Closing the embed player page failed", error);
            }),
        () => {
          // Opening failed and was reported above; there is nothing to close.
        },
      );
    };
  }, []);

  const origin = pageUrl === null ? null : playerOrigin(pageUrl, "embed");

  useEffect(() => {
    if (origin === null) return;
    const onMessage = (event: MessageEvent) => {
      if (event.origin !== origin || event.source !== frame.current?.contentWindow) return;
      const report = parseBridgeReport(event.data);
      if (report?.type === "error") {
        console.warn(`YouTube player error ${String(report.code)}`);
        setYouTubeError(report.code);
      }
    };
    window.addEventListener("message", onMessage);
    return () => {
      window.removeEventListener("message", onMessage);
    };
  }, [origin]);

  if (pageUrl === null || origin === null) return null;
  // Twitch's embed reports no errors, but it streams only in H.264.
  const failure: PlaybackFailure | null = isTwitchPlayer(playerUrl)
    ? { kind: "twitch" }
    : youTubeError === null
      ? null
      : { kind: "youtube", code: youTubeError };
  return (
    <>
      <iframe
        ref={frame}
        className="link-card__player"
        src={pageUrl}
        title={title}
        allow="autoplay; encrypted-media; fullscreen; picture-in-picture"
        allowFullScreen
        referrerPolicy="no-referrer"
        sandbox="allow-scripts allow-same-origin allow-presentation"
        onLoad={() => {
          const command: BridgeCommand = { type: "embed", url: playerUrl };
          frame.current?.contentWindow?.postMessage(command, origin);
        }}
      />
      <CodecHint failure={failure} className="link-card__codec-hint" />
    </>
  );
}
