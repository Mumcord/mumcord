/** Play/pause of the watch party for everyone in it; an icon button used in the controls and the collapsed bar. */
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi } from "../../api";
import { positionAt, type LiveParty } from "./watchPartyStore";

export function PlayPause({ serverId, live }: { serverId: number; live: LiveParty }) {
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
      className="button watch-party__icon"
      data-tooltip={label}
      aria-label={label}
      onClick={onClick}
    >
      <span aria-hidden="true">{playing ? "⏸" : "▶"}</span>
    </button>
  );
}
