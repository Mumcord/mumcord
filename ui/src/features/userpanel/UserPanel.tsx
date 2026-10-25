import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { useView } from "../../app/view";
import { getApi } from "../../api";
import { ConnectionInfo } from "../connection";
import { disconnect, useServers } from "../servers";
import { useAvatars } from "../members/useAvatars";
import { GAMEPAD_PATH, useNowGaming } from "../nowgaming";
import { trackLabel, useNowPlaying } from "../nowplaying";
import { describeError } from "./errors";
import { ProfileDialog } from "./ProfileDialog";
import { joinChannel, useReadOnly } from "./readOnly";
import { QuickSettings, type QuickSettingsState } from "./QuickSettings";
import { RegisterDialog } from "./RegisterDialog";
import { useMicLevelFill } from "./useMicLevelFill";
import { usePushToTalk } from "./usePushToTalk";
import { useUserPanel } from "./useUserPanel";
import "./userpanel.css";

type Dialog = "profile" | "register" | null;

function Icon({ d }: { d: string }) {
  return (
    <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" focusable="false">
      <path d={d} fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}

const MIC = "M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3zM6 11a6 6 0 0 0 12 0M12 17v4";
const MUSIC = "M9 18V5l12-2v13M9 18a3 3 0 1 1-6 0a3 3 0 1 1 6 0M21 16a3 3 0 1 1-6 0a3 3 0 1 1 6 0";
const HEADPHONES = "M4 15v-3a8 8 0 0 1 16 0v3M4 15h3v5H4zM17 15h3v5h-3z";
const GEAR =
  "M10.47 5.37L10.9 3.07L13.1 3.07L13.53 5.37L14.6 5.72L15.6 6.23L17.54 4.91L19.09 6.46L17.77 8.4L18.28 9.4L18.63 10.47L20.93 10.9L20.93 13.1L18.63 13.53L18.28 14.6L17.77 15.6L19.09 17.54L17.54 19.09L15.6 17.77L14.6 18.28L13.53 18.63L13.1 20.93L10.9 20.93L10.47 18.63L9.4 18.28L8.4 17.77L6.46 19.09L4.91 17.54L6.23 15.6L5.72 14.6L5.37 13.53L3.07 13.1L3.07 10.9L5.37 10.47L5.72 9.4L6.23 8.4L4.91 6.46L6.46 4.91L8.4 6.23L9.4 5.72ZM9 12a3 3 0 1 0 6 0a3 3 0 1 0-6 0";
const HANG_UP = "M3 14c5-5 13-5 18 0l-2.5 2.5-3-1.5v-2.5a9 9 0 0 0-5 0V15l-3 1.5z";
const SLASH = "M4 4l16 16";

export function UserPanel() {
  const { t } = useTranslation("userpanel");
  const { openSettings } = useView();
  const state = useUserPanel();
  const readOnly = useReadOnly();
  const self = state.self;
  const avatarOf = useAvatars(self ? [self] : []);
  const avatarSrc = self ? avatarOf(self) : null;
  const [menuOpen, setMenuOpen] = useState(false);
  const [quick, setQuick] = useState<QuickSettingsState | null>(null);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [error, setError] = useState<string | null>(null);
  const servers = useServers();
  const menuId = useId();
  const menuButton = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const previous = useRef<{ mute: boolean; deaf: boolean } | null>(null);
  const micButton = useRef<HTMLButtonElement>(null);

  const mute = self?.selfMute ?? false;
  const deaf = self?.selfDeaf ?? false;
  const present = self !== null;
  const ptt = usePushToTalk();
  const transmitting = self !== null && state.talking.has(self.session);
  // The own shared track (only reported while "Now playing" is on); no session matches -1.
  const ownTrack = useNowPlaying(self?.session ?? -1);
  const ownGame = useNowGaming(self?.session ?? -1);
  // Push-to-talk and not held: the mic is closed, so show it crossed out and hide the level.
  const pttIdle = ptt.active && !transmitting && !mute && !deaf;
  const showLevel = state.connected && !mute && !deaf && !pttIdle;
  const micTitle = ptt.active
    ? ptt.key === null
      ? t("pttNoKey")
      : t("ptt", { key: ptt.key })
    : undefined;
  useMicLevelFill(micButton, showLevel);

  // Announce state changes (also those made elsewhere, e.g. the server or a shortcut).
  useEffect(() => {
    if (!present) {
      previous.current = null;
      return;
    }
    const before = previous.current;
    previous.current = { mute, deaf };
    if (!before) return;
    if (before.deaf !== deaf) announce(deaf ? t("announce.deafened") : t("announce.undeafened"));
    else if (before.mute !== mute) announce(mute ? t("announce.muted") : t("announce.unmuted"));
  }, [present, mute, deaf, t]);

  // FR-1.47: the default avatar could not fit this server's image limit.
  useEffect(
    () =>
      getApi().on("defaultAvatarSkipped", () => {
        setError(t("profile.skipped", { ns: "settings" }));
      }),
    [t],
  );

  useEffect(() => {
    if (menuOpen) menuRef.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }, [menuOpen]);

  const run = (action: Promise<unknown>) => {
    setError(null);
    action.catch((e: unknown) => {
      setError(describeError(t, e));
    });
  };

  const closeMenu = () => {
    setMenuOpen(false);
    menuButton.current?.focus();
  };

  const closeDialog = () => {
    setDialog(null);
    menuButton.current?.focus();
  };

  const open = (which: Dialog) => {
    setMenuOpen(false);
    setDialog(which);
  };

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const items = [...event.currentTarget.querySelectorAll<HTMLElement>('[role="menuitem"]')];
    const index = items.indexOf(document.activeElement as HTMLElement);
    const move = (to: number) => {
      event.preventDefault();
      items[(to + items.length) % items.length]?.focus();
    };
    if (event.key === "ArrowDown") move(index + 1);
    else if (event.key === "ArrowUp") move(index - 1);
    else if (event.key === "Home") move(0);
    else if (event.key === "End") move(items.length - 1);
    else if (event.key === "Escape" || event.key === "Tab") {
      if (event.key === "Escape") event.preventDefault();
      closeMenu();
    }
  };

  const name = self?.name ?? state.identityName ?? t("unknownName");
  const connected = state.connected;
  const link = servers.connection.state === "disconnected" ? null : servers.connection.serverId;
  const linked = servers.servers.find((s) => s.id === link);
  const serverLabel = linked ? linked.label || linked.address : "";
  const connection = servers.connection.state;
  const status =
    connection === "reconnecting"
      ? t("status.reconnecting")
      : connection === "connecting"
        ? t("status.connecting")
        : !connected
          ? t("status.disconnected")
          : readOnly
            ? t("status.readOnly")
            : null;
  // In the normal connected state the status line shows the own game or playing track. The game
  // wins the short line (it says more about being busy); the tooltip names both.
  const track = status === null && ownTrack?.track.playing ? trackLabel(ownTrack.track) : null;
  const game = status === null && ownGame ? ownGame.game.name : null;
  const statusText = status ?? game ?? track ?? (serverLabel || t("status.connected"));
  const statusTitle =
    game !== null && track !== null
      ? `${t("status.playing", { game })} · ${t("status.listening", { track })}`
      : statusText;

  return (
    <div className="userpanel">
      <div className="userpanel__identity">
        {/* Named by its content, the visible name and status line, so speech input users can say
            what they see (WCAG 2.5.3 label in name). */}
        <button
          ref={menuButton}
          type="button"
          className="userpanel__user"
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          aria-controls={menuOpen ? menuId : undefined}
          disabled={!connected}
          onClick={() => {
            setMenuOpen((o) => !o);
          }}
        >
          {/* The initial is drawn by CSS from data-initial, not as DOM text, so the button shows no
              visible text that is missing from its name (WCAG 2.5.3). */}
          <span
            className="userpanel-avatar"
            data-initial={avatarSrc ? undefined : name.slice(0, 1).toUpperCase()}
            aria-hidden="true"
          >
            {avatarSrc ? <img className="userpanel-avatar__img" src={avatarSrc} alt="" /> : null}
          </span>
          <span className="userpanel__text">
            <span className="userpanel__name" title={name}>
              {name}
            </span>{" "}
            <span className="userpanel__status" title={statusTitle}>
              {(game ?? track) !== null && (
                <svg
                  className="userpanel__status-icon"
                  viewBox="0 0 24 24"
                  aria-hidden="true"
                  focusable="false"
                >
                  <path
                    d={game !== null ? GAMEPAD_PATH : MUSIC}
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  />
                </svg>
              )}
              {statusText}
            </span>
          </span>
        </button>
      </div>
      <div className="userpanel__row">
        <button
          ref={micButton}
          type="button"
          className="userpanel__toggle userpanel__toggle--mic"
          data-ptt-idle={pttIdle ? "" : undefined}
          aria-pressed={mute}
          aria-label={t("mute")}
          title={micTitle}
          disabled={!connected}
          onClick={() => {
            run(getApi().setSelfMute(!mute));
          }}
        >
          {showLevel ? <span className="userpanel__level" aria-hidden="true" /> : null}
          <span className="userpanel__icon">
            <Icon d={MIC} />
            {mute || pttIdle ? <Icon d={SLASH} /> : null}
          </span>
        </button>
        <button
          type="button"
          className="userpanel__toggle"
          aria-pressed={deaf}
          aria-label={t("deafen")}
          disabled={!connected}
          onClick={() => {
            run(getApi().setSelfDeaf(!deaf));
          }}
        >
          <span className="userpanel__icon">
            <Icon d={HEADPHONES} />
            {deaf ? <Icon d={SLASH} /> : null}
          </span>
        </button>
        {link !== null ? (
          <button
            type="button"
            className="userpanel__toggle userpanel__toggle--hangup"
            aria-label={t("disconnect", { server: serverLabel })}
            title={t("disconnect", { server: serverLabel })}
            onClick={() => {
              void disconnect();
            }}
          >
            <Icon d={HANG_UP} />
          </button>
        ) : null}
        {connected ? <ConnectionInfo className="userpanel__toggle" /> : null}
        <button
          type="button"
          className="userpanel__toggle"
          aria-label={t("settings")}
          onClick={openSettings}
          onContextMenu={(event) => {
            event.preventDefault();
            setQuick({ x: event.clientX, y: event.clientY, opener: event.currentTarget });
          }}
          onKeyDown={(event) => {
            if (event.key !== "ContextMenu" && !(event.key === "F10" && event.shiftKey)) return;
            event.preventDefault();
            const box = event.currentTarget.getBoundingClientRect();
            setQuick({ x: box.left, y: box.bottom, opener: event.currentTarget });
          }}
        >
          <Icon d={GEAR} />
        </button>
      </div>
      {quick ? (
        <QuickSettings
          menu={quick}
          onOpenSettings={openSettings}
          onClose={() => {
            setQuick(null);
          }}
        />
      ) : null}
      {menuOpen ? (
        <div
          tabIndex={-1}
          id={menuId}
          ref={menuRef}
          role="menu"
          className="userpanel__menu"
          aria-label={t("menu")}
          onKeyDown={onMenuKeyDown}
        >
          <div role="none">
            <button
              type="button"
              role="menuitem"
              tabIndex={-1}
              onClick={() => {
                open("profile");
              }}
            >
              {t("menuProfile")}
            </button>
          </div>
          {state.canRegister ? (
            <div role="none">
              <button
                type="button"
                role="menuitem"
                tabIndex={-1}
                onClick={() => {
                  open("register");
                }}
              >
                {t("menuRegister")}
              </button>
            </div>
          ) : null}
        </div>
      ) : null}
      {connected && readOnly && self ? (
        <div className="userpanel-readonly" role="status">
          <p className="userpanel-readonly__text">{t("readOnly.text")}</p>
          {state.channelName !== null ? (
            <button
              type="button"
              className="button userpanel-readonly__join"
              onClick={() => {
                run(joinChannel(self.channelId));
              }}
            >
              {t("readOnly.join", { channel: state.channelName })}
            </button>
          ) : null}
        </div>
      ) : null}
      {error ? (
        <p className="userpanel-error" role="alert">
          {error}
        </p>
      ) : null}
      {dialog === "profile" && self ? (
        <ProfileDialog
          session={self.session}
          hasAvatar={self.hasAvatar}
          registered={self.registered}
          onClose={closeDialog}
        />
      ) : null}
      {dialog === "register" ? <RegisterDialog onClose={closeDialog} /> : null}
    </div>
  );
}
