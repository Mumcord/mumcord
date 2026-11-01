/**
 * The channel header while the watch party's cinema view is open (issue #163): a "Watch together · n"
 * pill next to the channel name, and "Mini player" (back to the chat, the party goes on in the mini
 * bar) and "Leave party" with the other header actions. The header itself stays the channel's.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { MediaIcon } from "../media/icons";
import { leaveCinema, useMediaView } from "../media/mediaView";
import { leaveWatchParty } from "./startParty";
import { useWatchParty } from "./watchPartyStore";
import "./watchParty.css";

function useCinemaParty(serverId: number) {
  const view = useMediaView();
  const live = useWatchParty(serverId);
  return view.cinema === "watch" && live !== null && live.party.joined ? live.party : null;
}

/** The channel title with the party pill beside it; just the title while the cinema is closed. */
export function WatchHeaderTitle({
  serverId,
  children,
}: {
  serverId: number;
  children: ReactNode;
}): ReactNode {
  const { t } = useTranslation("watchparty");
  const party = useCinemaParty(serverId);
  if (party === null) return children;
  return (
    <div className="watch-header">
      {children}
      <span className="watch-header__pill">
        <MediaIcon name="play" size={10} />
        {t("headerPill", { count: party.participants.length })}
      </span>
    </div>
  );
}

export function WatchHeaderActions({ serverId }: { serverId: number }): ReactNode {
  const { t } = useTranslation("watchparty");
  const party = useCinemaParty(serverId);
  if (party === null) return null;
  return (
    <>
      <button type="button" className="button watch-header__action" onClick={leaveCinema}>
        <MediaIcon name="mini" size={14} />
        {t("miniPlayer")}
      </button>
      <button
        type="button"
        className="button watch-header__action watch-header__action--leave"
        onClick={() => {
          leaveWatchParty(serverId, t("leaveFailed"));
        }}
      >
        {t("leave")}
      </button>
    </>
  );
}
