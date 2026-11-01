/**
 * The party's player as both the cinema view and the mini bar show it (issue #163). Only one of the
 * two is open at a time, and each follows the party through {@link useWatchPlayer}, so the video
 * keeps running in the mini bar and both show the same state. Only the app's own controls drive the
 * player: the frame takes no clicks, and YouTube's controls are off in the player page.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { loadCommand, playerOrigin } from "./playerBridge";
import type { PartyPlayer } from "./usePartyPlayer";
import type { LiveParty } from "./watchPartyStore";

/** The player page's frame, or nothing until its URL is known or after it failed. */
export function PartyPlayerFrame({
  live,
  player,
  frameRef,
  className,
}: PartyPlayer & { live: LiveParty; className: string }): ReactNode {
  const { t } = useTranslation("watchparty");
  const origin = playerOrigin(player.pageUrl ?? "");
  const videoId = live.party.videoId;
  if (origin === null || player.failed) return null;
  return (
    <iframe
      key={videoId}
      ref={frameRef}
      className={className}
      src={player.pageUrl ?? ""}
      title={t("player")}
      allow="autoplay; encrypted-media; fullscreen; picture-in-picture"
      referrerPolicy="no-referrer"
      sandbox="allow-scripts allow-same-origin allow-presentation"
      // Keyboard focus stays with the app's controls, like the clicks.
      tabIndex={-1}
      data-ready={player.ready}
      onLoad={() => {
        frameRef.current?.contentWindow?.postMessage(loadCommand(videoId), origin);
      }}
    />
  );
}
