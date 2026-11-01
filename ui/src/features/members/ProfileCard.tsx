/**
 * Full profile of one person (modal dialog, FR-1.63): a header with banner, avatar, name, status
 * and comment, "Private message" as a labelled header action, and the local settings (volume
 * 0–200 %, local mute, friend, private note). Settings apply instantly and confirm with "Saved"; there is
 * no save button, so nothing at the bottom can be mistaken for one (QA.63). The engine stores
 * them per certificate hash; people without a certificate are only adjusted for this session.
 */
import { PluginProfileSections } from "../plugins/badges";
import DOMPurify from "dompurify";
import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
} from "react";
import { useTranslation } from "react-i18next";
import { Disclosure } from "../../components/Disclosure";
import { useThrottledSend } from "../../components/useThrottledSend";
import { announce } from "../../a11y";
import { getApi, toEngineError, type User } from "../../api";
import { shownServerId, useServerStateOf, useShownServerId } from "../channels/serverStore";
import { setFriend, useFriend } from "../friends";
import { MumcordBadge } from "../channels/MumcordBadge";
import { ConnectionQuality } from "../connection/ConnectionQuality";
import { NowGamingFor } from "../nowgaming";
import { PositionalBadge } from "../positional";
import { NowPlayingFor } from "../nowplaying";
import "./members.css";
import { Avatar } from "./Avatar";
import { avatarColorVar, useAvatarColor } from "./avatarColor";
import { openPrivateConversation, type SendPrivateMessage } from "./privateMessage";
import { usePersonProfile } from "./usePersonProfile";

/** Delay before a slider change is sent, so dragging doesn't fire a command per pixel. */
/** Minimum gap between volume commands while dragging (about 20 per second). */
export const VOLUME_THROTTLE_MS = 50;
/** Delay after the last keystroke before the private note is saved. */
export const NOTE_DEBOUNCE_MS = 600;
/** How long "Saved" stays visible after a change was stored. */
export const SAVED_VISIBLE_MS = 2000;
/** Longest private note; the engine rejects longer ones. */
const MAX_NOTE_LENGTH = 1000;

export interface ProfileCardProps {
  session: number;
  /** Server the person is on; the shown server when omitted. */
  serverId?: number | undefined;
  /** Called when the card should close (Escape, close button, person gone). */
  onClose: () => void;
  /** Element that gets focus back after closing (usually the one that opened the card). */
  returnFocus?: HTMLElement | null | undefined;
  /** "Private message"; defaults to opening the private conversation in the chat. */
  onSendPrivateMessage?: SendPrivateMessage | undefined;
  /** Scrolls to and focuses the connection figures when the card opens. */
  focusConnection?: boolean | undefined;
}

export function ProfileCard({
  session,
  serverId,
  onClose: onCloseProp,
  returnFocus = null,
  onSendPrivateMessage = openPrivateConversation,
  focusConnection = false,
}: ProfileCardProps) {
  const shown = useShownServerId();
  const state = useServerStateOf(serverId ?? shown);
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
    if (present && dialog && !dialog.open) {
      dialog.showModal();
      if (focusConnection) {
        const section = dialog.querySelector<HTMLElement>("[data-connection-section]");
        // jsdom has no scrollIntoView.
        // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition
        section?.scrollIntoView?.({ block: "center" });
        (section?.querySelector<HTMLElement>("button") ?? section)?.focus();
      }
    }
  }, [present, focusConnection]);

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
        serverId={serverId}
        titleId={titleId}
        isSelf={isSelf}
        talking={state.talking.has(session)}
        onSendPrivateMessage={() => {
          if (serverId === undefined) onSendPrivateMessage(user);
          else onSendPrivateMessage(user, serverId);
          onClose();
        }}
        onClose={onClose}
      />
    </dialog>
  );
}

function ProfileBody({
  user,
  serverId,
  titleId,
  isSelf,
  talking,
  onSendPrivateMessage,
  onClose,
}: {
  user: User;
  serverId: number | undefined;
  titleId: string;
  isSelf: boolean;
  talking: boolean;
  onSendPrivateMessage: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation("members");
  const { profile, loadError } = usePersonProfile(user.session, serverId);
  const [error, setError] = useState<string | null>(null);
  const [saved, showSaved] = useSavedFlash(t("card.saved"));
  const avatarColor = useAvatarColor(profile?.avatar ?? null);
  const settingsHint = user.localMute
    ? t("card.hintMuted")
    : user.volume !== 100
      ? t("card.hintVolume", { value: user.volume })
      : undefined;
  const muteLabelId = useId();
  const muteHintId = useId();
  const friendLabelId = useId();
  const friendHintId = useId();
  const { t: tFriends } = useTranslation("friends");
  const friend = useFriend(user.certHash);

  const report = (cause: unknown) => {
    const code = toEngineError(cause).code;
    setError(code === "notConnected" ? t("errors.notConnected") : t("errors.generic"));
  };
  const stored = () => {
    setError(null);
    showSaved();
  };

  const comment = profile?.comment ? DOMPurify.sanitize(profile.comment) : "";

  return (
    <>
      <div
        className="profile-card__banner"
        style={{ "--banner-bg": avatarColor ?? avatarColorVar(user.name) } as CSSProperties}
      >
        <div className="profile-card__actions">
          {!isSelf && (
            <button type="button" className="profile-card__action" onClick={onSendPrivateMessage}>
              <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" focusable="false">
                <path
                  d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinejoin="round"
                />
              </svg>
              {t("card.privateMessage")}
            </button>
          )}
          <button
            type="button"
            className="profile-card__action profile-card__action--icon"
            aria-label={t("card.close")}
            data-tooltip={t("card.close")}
            onClick={onClose}
          >
            {/* Cross; the button's name is its translated aria-label. */}
            <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" focusable="false">
              <path
                d="M6 6l12 12M18 6L6 18"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
              />
            </svg>
          </button>
        </div>
      </div>
      <div className="profile-card__header">
        <Avatar name={user.name} src={profile?.avatar ?? null} talking={talking} size="lg" />
        <h2 id={titleId} className="profile-card__name">
          {user.name}
          <MumcordBadge user={user} />
          <PositionalBadge session={user.session} serverId={serverId} />
        </h2>
        <p className="profile-card__meta">
          {user.registered ? t("registered") : t("unregistered")}
          {talking && <> · {t("talking")}</>}
        </p>
      </div>
      <div data-connection-section tabIndex={-1} className="profile-card__connection">
        <ConnectionQuality session={user.session} serverId={serverId} />
      </div>
      {loadError && <p className="profile-card__note">{t("card.loadFailed")}</p>}
      <NowGamingFor session={user.session} serverId={serverId} joinable={!isSelf} />
      <NowPlayingFor session={user.session} serverId={serverId} />
      <PluginProfileSections session={user.session} />
      {comment && (
        <div
          className="profile-card__comment"
          // Sanitized again here: server comments are untrusted even after the engine's pass.
          dangerouslySetInnerHTML={{ __html: comment }}
        />
      )}
      {!isSelf && (
        <section className="profile-card__section">
          <Disclosure title={t("card.settingsFor", { name: user.name })} hint={settingsHint}>
            <div className="profile-card__section-head">
              {/* Screen readers hear "Saved" through the announcer instead. */}
              <span className="profile-card__saved" data-visible={saved} aria-hidden="true">
                {t("card.saved")}
              </span>
            </div>
            <p className="profile-card__note">{t("card.localSettingsHint")}</p>
            <VolumeSlider user={user} serverId={serverId} onError={report} onSaved={stored} />
            <button
              type="button"
              role="switch"
              aria-checked={user.localMute}
              aria-labelledby={muteLabelId}
              aria-describedby={muteHintId}
              className="profile-card__mute"
              onClick={() => {
                setError(null);
                getApi()
                  .personSetLocalMute(serverId ?? shownServerId(), user.session, !user.localMute)
                  .then(stored, report);
              }}
            >
              <span className="profile-card__mute-text">
                <span id={muteLabelId}>{t("card.localMute")}</span>
                <span id={muteHintId} className="profile-card__note">
                  {t("card.localMuteHint")}
                </span>
              </span>
              <span className="profile-card__switch" aria-hidden="true" />
            </button>
            {/* People without a certificate can't be recognised again, so they can't be friends. */}
            <button
              type="button"
              role="switch"
              aria-checked={friend !== undefined}
              aria-disabled={user.certHash === null}
              aria-labelledby={friendLabelId}
              aria-describedby={friendHintId}
              className="profile-card__mute"
              onClick={() => {
                if (user.certHash === null) return;
                setError(null);
                setFriend(user, friend === undefined, serverId).then(stored, report);
              }}
            >
              <span className="profile-card__mute-text">
                <span id={friendLabelId}>{tFriends("friend")}</span>
                <span id={friendHintId} className="profile-card__note">
                  {user.certHash === null ? tFriends("noCertificate") : tFriends("friendHint")}
                </span>
              </span>
              <span className="profile-card__switch" aria-hidden="true" />
            </button>
            {profile !== null && (
              <NoteField
                session={user.session}
                serverId={serverId}
                initial={profile.note}
                onError={report}
                onSaved={stored}
              />
            )}
            {user.certHash === null && (
              <p className="profile-card__note">{t("card.sessionOnly")}</p>
            )}
          </Disclosure>
        </section>
      )}
      {error && (
        <p role="alert" className="profile-card__error">
          {error}
        </p>
      )}
    </>
  );
}

/** Visible "Saved" flag for a short while after each stored change, also announced. */
function useSavedFlash(message: string): [boolean, () => void] {
  const [visible, setVisible] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  const show = useCallback(() => {
    setVisible(true);
    announce(message);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      setVisible(false);
    }, SAVED_VISIBLE_MS);
  }, [message]);

  return [visible, show];
}

/**
 * Sends the last scheduled change after `delayMs` of quiet; `flush` sends it at once. A change
 * still pending when the card closes is sent then, so nothing typed or dragged is lost.
 */
function useDebouncedSend(delayMs: number): {
  schedule: (send: () => void) => void;
  flush: () => void;
} {
  const pending = useRef<{ timer: ReturnType<typeof setTimeout>; send: () => void } | null>(null);

  const flush = useCallback(() => {
    if (pending.current) {
      clearTimeout(pending.current.timer);
      pending.current.send();
    }
  }, []);

  useEffect(() => flush, [flush]);

  const schedule = useCallback(
    (send: () => void) => {
      if (pending.current) clearTimeout(pending.current.timer);
      const run = () => {
        pending.current = null;
        send();
      };
      pending.current = { timer: setTimeout(run, delayMs), send: run };
    },
    [delayMs],
  );

  return { schedule, flush };
}

function NoteField({
  session,
  serverId,
  initial,
  onError,
  onSaved,
}: {
  session: number;
  serverId: number | undefined;
  initial: string;
  onError: (cause: unknown) => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation("members");
  const inputId = useId();
  const hintId = useId();
  const [value, setValue] = useState(initial);
  const { schedule, flush } = useDebouncedSend(NOTE_DEBOUNCE_MS);

  return (
    <div className="profile-card__field">
      <label htmlFor={inputId}>{t("card.privateNote")}</label>
      <textarea
        id={inputId}
        className="profile-card__textarea"
        rows={3}
        maxLength={MAX_NOTE_LENGTH}
        value={value}
        aria-describedby={hintId}
        onChange={(event) => {
          const note = event.target.value;
          setValue(note);
          schedule(() => {
            getApi()
              .personSetNote(serverId ?? shownServerId(), session, note)
              .then(onSaved, onError);
          });
        }}
        onBlur={flush}
      />
      <p id={hintId} className="profile-card__note">
        {t("card.privateNoteHint")}
      </p>
    </div>
  );
}

export function VolumeSlider({
  user,
  serverId,
  onError,
  onSaved,
}: {
  user: User;
  /** Server the person is on; the shown server when omitted. */
  serverId?: number | undefined;
  onError: (cause: unknown) => void;
  /** Called after the engine stored a change. */
  onSaved?: () => void;
}) {
  const { t } = useTranslation("members");
  const inputId = useId();
  const [value, setValue] = useState(user.volume);
  const { schedule } = useThrottledSend(VOLUME_THROTTLE_MS);

  const change = (volume: number) => {
    setValue(volume);
    schedule(() => {
      getApi()
        .personSetVolume(serverId ?? shownServerId(), user.session, volume)
        .then(() => onSaved?.(), onError);
    });
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
