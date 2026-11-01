/** Play/pause of the watch party for everyone in it; an icon button used in the controls and the collapsed bar. */
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi } from "../../api";
import { MediaIcon } from "../media/icons";
import { positionAt, type LiveParty } from "./watchPartyStore";

export function PlayPause({
  serverId,
  live,
  className = "button watch-party__icon",
}: {
  serverId: number;
  live: LiveParty;
  /** The look of the place it sits in: the picture's overlay, the mini bar or the playlist row. */
  className?: string;
}) {
  const { t } = useTranslation("watchparty");
  const { playing } = live.party;
  const label = playing ? t("widget.pause") : t("widget.play");
  const onClick = () => {
    const positionMs = Math.round(positionAt(live, Date.now()));
    getApi()
      .watchPartyCommand(serverId, { kind: playing ? "pause" : "play", positionMs })
      .catch((error: unknown) => {
        console.error(t("commandFailed"), error);
        announce(t("commandFailed"), "assertive");
      });
  };
  return (
    <button
      type="button"
      className={className}
      data-tooltip={label}
      aria-label={label}
      aria-keyshortcuts="Space"
      onClick={onClick}
    >
      <MediaIcon name={playing ? "pause" : "play"} />
    </button>
  );
}
