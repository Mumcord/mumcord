/**
 * Profile card for one person (modal dialog): avatar, name, comment, and local controls (volume
 * 0–200 %, local mute, send private message). Volume and mute are stored by the engine per
 * certificate hash; people without a certificate are only adjusted for this session.
 */
import DOMPurify from "dompurify";
import { useCallback, useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { getApi, toEngineError, type User } from "../../api";
import { useServerState } from "../channels/serverStore";
import "./members.css";
import { Avatar } from "./Avatar";
import { openPrivateConversation, type SendPrivateMessage } from "./privateMessage";
import { usePersonProfile } from "./usePersonProfile";

/** Delay before a slider change is sent, so dragging doesn't fire a command per pixel. */
export const VOLUME_DEBOUNCE_MS = 150;

export interface ProfileCardProps {
  session: number;
  /** Called when the card should close (Escape, close button, person gone). */
  onClose: () => void;
  /** Element that gets focus back after closing (usually the one that opened the card). */
  returnFocus?: HTMLElement | null | undefined;
  /** "Send private message"; defaults to opening the private conversation in the chat. */
  onSendPrivateMessage?: SendPrivateMessage | undefined;
}

export function ProfileCard({
  session,
  onClose: onCloseProp,
  returnFocus = null,
  onSendPrivateMessage = openPrivateConversation,
}: ProfileCardProps) {
  const { t } = useTranslation("members");
  const state = useServerState();
  const user = state.users.get(session);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const titleId = useId();

  const onClose = useCallback(() => {
    onCloseProp();
    // The opener may have re-rendered away (e.g. person left); only refocus if still attached.
    if (returnFocus?.isConnected) returnFocus.focus();
  }, [onCloseProp, returnFocus]);

  const present = user !== undefined;
  useEffect(() => {
    const dialog = dialogRef.current;
    if (present && dialog && !dialog.open) dialog.showModal();
  }, [present]);

  // The person left or the connection dropped: nothing left to show.
  useEffect(() => {
    if (!user) onClose();
  }, [user, onClose]);

  if (!user) return null;

  const onKeyDown = (event: KeyboardEvent<HTMLDialogElement>) => {
    if (event.key === "Escape") {
      // Prevents the native cancel so closing always goes through onClose (focus return).
      event.preventDefault();
      event.stopPropagation();
      onClose();
    }
  };

  const isSelf = session === state.selfSession;
  return (
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- Escape handling for the modal dialog
    <dialog
      ref={dialogRef}
      className="profile-card"
      aria-labelledby={titleId}
      aria-modal="true"
      onKeyDown={onKeyDown}
    >
      <ProfileBody
        key={session}
        user={user}
        titleId={titleId}
        isSelf={isSelf}
        talking={state.talking.has(session)}
        onSendPrivateMessage={() => {
          onSendPrivateMessage(user);
          onClose();
        }}
      />
      <button type="button" className="profile-card__close" onClick={onClose}>
        {t("card.close")}
      </button>
    </dialog>
  );
}

function ProfileBody({
  user,
  titleId,
  isSelf,
  talking,
  onSendPrivateMessage,
}: {
  user: User;
  titleId: string;
  isSelf: boolean;
  talking: boolean;
  onSendPrivateMessage: () => void;
}) {
  const { t } = useTranslation("members");
  const { profile, loadError } = usePersonProfile(user.session);
  const [error, setError] = useState<string | null>(null);

  const report = (cause: unknown) => {
    const code = toEngineError(cause).code;
    setError(code === "notConnected" ? t("errors.notConnected") : t("errors.generic"));
  };

  const comment = profile?.comment ? DOMPurify.sanitize(profile.comment) : "";

  return (
    <>
      <div className="profile-card__header">
        <Avatar name={user.name} src={profile?.avatar ?? null} talking={talking} size="lg" />
        <div>
          <h2 id={titleId} className="profile-card__name">
            {user.name}
          </h2>
          <p className="profile-card__meta">
            {user.registered ? t("registered") : t("unregistered")}
            {talking && <> · {t("talking")}</>}
          </p>
        </div>
      </div>
      {loadError && <p className="profile-card__note">{t("card.loadFailed")}</p>}
      {comment && (
        <div
          className="profile-card__comment"
          // Sanitized again here: server comments are untrusted even after the engine's pass.
          dangerouslySetInnerHTML={{ __html: comment }}
        />
      )}
      {!isSelf && (
        <>
          <VolumeSlider user={user} onError={report} />
          <label className="profile-card__mute">
            <input
              type="checkbox"
              checked={user.localMute}
              onChange={(event) => {
                setError(null);
                getApi().personSetLocalMute(user.session, event.target.checked).catch(report);
              }}
            />
            {t("card.localMute")}
          </label>
          {user.certHash === null && <p className="profile-card__note">{t("card.sessionOnly")}</p>}
          <button type="button" className="profile-card__pm" onClick={onSendPrivateMessage}>
            {t("card.sendPrivateMessage")}
          </button>
        </>
      )}
      {error && (
        <p role="alert" className="profile-card__error">
          {error}
        </p>
      )}
    </>
  );
}

function VolumeSlider({ user, onError }: { user: User; onError: (cause: unknown) => void }) {
  const { t } = useTranslation("members");
  const inputId = useId();
  const [value, setValue] = useState(user.volume);
  const pending = useRef<{ timer: ReturnType<typeof setTimeout>; send: () => void } | null>(null);

  // Sends a pending change when the card closes mid-debounce.
  useEffect(
    () => () => {
      if (pending.current) {
        clearTimeout(pending.current.timer);
        pending.current.send();
      }
    },
    [],
  );

  const change = (volume: number) => {
    setValue(volume);
    if (pending.current) clearTimeout(pending.current.timer);
    const send = () => {
      pending.current = null;
      getApi().personSetVolume(user.session, volume).catch(onError);
    };
    pending.current = { timer: setTimeout(send, VOLUME_DEBOUNCE_MS), send };
  };

  return (
    <div className="profile-card__volume">
      <label htmlFor={inputId}>{t("card.volume")}</label>
      <input
        id={inputId}
        type="range"
        min={0}
        max={200}
        step={5}
        value={value}
        aria-valuetext={t("card.volumeValue", { value })}
        onChange={(event) => {
          change(Number(event.target.value));
        }}
      />
      <output htmlFor={inputId}>{t("card.volumeValue", { value })}</output>
    </div>
  );
}
