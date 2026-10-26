/**
 * Dialog to start a watch party or, while one runs in the user's channel, add a video to its
 * playlist (FR-2.65, FR-2.69). Shared by the Apps menu and the party widget's "+" button.
 */
import { useId, useState, type SyntheticEvent } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { useServerStateOf } from "../channels";
import { ModalDialog } from "../settings/components/ModalDialog";
import { Playlist } from "./Playlist";
import { addToPlaylist } from "./playlistActions";
import { startWatchParty } from "./startParty";
import { useWatchParty } from "./watchPartyStore";
import { videoIdOfLink } from "./youtube";

export function WatchLinkDialog({ serverId, onClose }: { serverId: number; onClose: () => void }) {
  const { t } = useTranslation("chat");
  const { t: tParty } = useTranslation("watchparty");
  const { users, selfSession, connected } = useServerStateOf(serverId);
  const live = useWatchParty(serverId);
  const [link, setLink] = useState("");
  const [invalid, setInvalid] = useState(false);
  const errorId = useId();
  const linkId = useId();
  const ownChannel = selfSession === null ? undefined : users.get(selfSession)?.channelId;
  const partyRunning = live !== null && live.party.channelId === ownChannel;

  const submit = (event: SyntheticEvent) => {
    event.preventDefault();
    if (!connected) return;
    const videoId = videoIdOfLink(link);
    if (videoId === null) {
      setInvalid(true);
      announce(t("actions.invalidLink"), "assertive");
      return;
    }
    setInvalid(false);
    // Starting or adding never posts a chat message (FR-2.65, FR-2.73). A running party is
    // never restarted; the link only goes to its playlist (FR-2.69).
    if (partyRunning) addToPlaylist(serverId, videoId, tParty("playlist.addFailed"));
    else startWatchParty(serverId, videoId, tParty("startFailed"));
    setLink("");
    onClose();
  };

  return (
    <ModalDialog
      title={partyRunning ? t("actions.addTitle") : t("actions.dialogTitle")}
      onClose={onClose}
    >
      <form onSubmit={submit} noValidate>
        <label htmlFor={`${linkId}-link`} className="settings-field">
          {t("actions.linkLabel")}
        </label>
        <input
          id={`${linkId}-link`}
          type="text"
          className="settings-input"
          aria-invalid={invalid || undefined}
          aria-describedby={invalid ? errorId : undefined}
          placeholder={t("actions.linkPlaceholder")}
          value={link}
          onChange={(event) => {
            setLink(event.target.value);
            setInvalid(false);
          }}
        />
        {invalid && (
          <p id={errorId} className="chat-actions__error" role="alert">
            {t("actions.invalidLink")}
          </p>
        )}
        {live !== null && partyRunning && live.party.joined && (
          <Playlist serverId={serverId} party={live.party} users={users} />
        )}
        <div className="settings-dialog__actions">
          <button type="button" className="settings-button" onClick={onClose}>
            {t("common:cancel")}
          </button>
          <button type="submit" className="settings-button settings-button--primary">
            {partyRunning ? t("actions.addConfirm") : t("actions.confirm")}
          </button>
        </div>
      </form>
    </ModalDialog>
  );
}
