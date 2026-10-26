/**
 * "Watch together" on a YouTube card (FR-2.39): shown only in the chat of the user's own channel
 * while no party runs there; starting invites the Mumcord users in that channel. While the user
 * is in a party there, the card offers "Add to playlist" instead (FR-2.69); the link already is
 * a chat message, so nothing is posted again.
 */
import { useTranslation } from "react-i18next";
import { useSelectedConversation, useServerState, useShownServerId } from "../channels";
import { addToPlaylist } from "./playlistActions";
import { startWatchParty } from "./startParty";
import { useWatchParty } from "./watchPartyStore";
import { videoIdOf } from "./youtube";

interface WatchTogetherButtonProps {
  /** The card's YouTube embed URL; anything else shows no button. */
  playerUrl: string;
  title: string;
  tabIndex: number;
}

export function WatchTogetherButton({ playerUrl, title, tabIndex }: WatchTogetherButtonProps) {
  const { t } = useTranslation("watchparty");
  const serverId = useShownServerId();
  const { conversation } = useSelectedConversation();
  const { users, selfSession } = useServerState();
  const live = useWatchParty(serverId);
  const videoId = videoIdOf(playerUrl);
  const ownChannel = selfSession === null ? undefined : users.get(selfSession)?.channelId;
  const inOwnChannel = conversation?.kind === "channel" && conversation.channelId === ownChannel;
  if (serverId === null || videoId === null || !inOwnChannel) return null;
  if (live?.party.channelId === ownChannel) {
    if (!live.party.joined) return null;
    return (
      <button
        type="button"
        className="button link-card__watch-together"
        tabIndex={tabIndex}
        aria-label={t("addToPlaylistFor", { title })}
        onClick={() => {
          addToPlaylist(serverId, videoId, t("playlist.addFailed"));
        }}
      >
        {t("addToPlaylist")}
      </button>
    );
  }
  const start = () => {
    startWatchParty(serverId, videoId, t("startFailed"));
  };
  return (
    <button
      type="button"
      className="button link-card__watch-together"
      tabIndex={tabIndex}
      aria-label={t("watchTogetherFor", { title })}
      onClick={start}
    >
      {t("watchTogether")}
    </button>
  );
}
