import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { useView } from "../../app/view";
import { getApi } from "../../api";
import { showServer, useServers as useServerStates, useShownServerId } from "../channels";
import { ConfirmDialog, disconnect, useServers } from "../servers";
import type { MenuPosition } from "../../components/ContextMenu";
import { useAvatars } from "../members/useAvatars";
import { GAMEPAD_PATH, useNowGaming } from "../nowgaming";
import { trackLabel, useNowPlaying } from "../nowplaying";
import { DisconnectMenu } from "./DisconnectMenu";
import { describeError } from "./errors";
import { ProfileDialog } from "./ProfileDialog";
import { joinChannel } from "./readOnly";
import { QuickSettings, type QuickSettingsState } from "./QuickSettings";
import { RegisterDialog } from "./RegisterDialog";
import { ArrowIcon, AudioArrow } from "./AudioArrow";
import { StatusStrip } from "./StatusStrip";
import { stripMessage } from "./stripMessage";
import { useAudioPopover } from "./useAudioPopover";
import { useMicLevelFill } from "./useMicLevelFill";
import { usePushToTalk } from "./usePushToTalk";
import { useUserPanel } from "./useUserPanel";
import { onUiHotkey } from "../hotkeys";
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
  const { openSettings, openAudioSettings } = useView();
  const state = useUserPanel();
  const shownServerId = useShownServerId();
  const { self, shownSelf, voice } = state;
  const avatarOf = useAvatars(self ? [self] : []);
  const avatarSrc = self ? avatarOf(self) : null;
  const [quick, setQuick] = useState<QuickSettingsState | null>(null);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [error, setError] = useState<string | null>(null);
  const servers = useServers();
  const serverStates = useServerStates();
  const [disconnectMenu, setDisconnectMenu] = useState<MenuPosition | null>(null);
  const [confirmAll, setConfirmAll] = useState(false);
  const settingsButton = useRef<HTMLButtonElement>(null);
  const previous = useRef<{ mute: boolean; deaf: boolean } | null>(null);
  const micButton = useRef<HTMLButtonElement>(null);
  const audio = useAudioPopover(openAudioSettings);

  const mute = self?.selfMute ?? false;
  const deaf = self?.selfDeaf ?? false;
  const present = self !== null;
  const ptt = usePushToTalk();
  const transmitting = self !== null && state.talking.has(self.session);
  // The own shared track (only reported while "Now playing" is on); no session matches -1.
  const ownTrack = useNowPlaying(shownSelf?.session ?? -1);
  const ownGame = useNowGaming(shownSelf?.session ?? -1);
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

  const run = (action: Promise<unknown>) => {
    setError(null);
    action.catch((e: unknown) => {
      setError(describeError(t, e));
    });
  };

  const openDisconnectMenu = (x: number, y: number, opener: HTMLElement) => {
    setDisconnectMenu({ x, y, opener });
  };
  const openDisconnectMenuBelow = (opener: HTMLElement) => {
    const box = opener.getBoundingClientRect();
    openDisconnectMenu(box.left, box.bottom, opener);
  };

  const leaveVoice = () => {
    run(getApi().voiceLeave());
    announce(t("announce.leftVoice", { server: nameOf(voice?.serverId ?? null) }));
  };
  const disconnectShown = () => {
    void disconnect();
    announce(t("announce.disconnected", { server: serverLabel }));
  };
  const switchVoice = (serverId: number) => {
    const target = serverStates.get(serverId);
    const channelId =
      target?.selfSession == null ? undefined : target.users.get(target.selfSession)?.channelId;
    if (channelId === undefined) return;
    run(getApi().channelJoin(serverId, channelId));
  };
  const disconnectAll = async () => {
    setConfirmAll(false);
    const ids = connectedIds;
    for (const id of ids) await disconnect(id);
    announce(t("announce.disconnectedAll"));
  };

  const closeDialog = () => {
    setDialog(null);
    settingsButton.current?.focus();
  };

  const name = self?.name ?? state.identityName ?? t("unknownName");
  const connected = state.connected;
  const link = servers.connection.state === "disconnected" ? null : servers.connection.serverId;
  const nameOf = (id: number | null) => {
    const saved = servers.servers.find((s) => s.id === id);
    return saved ? saved.label || saved.address : "";
  };
  const serverLabel = nameOf(link);
  const connectedIds = [...servers.connections.keys()];
  const switchTargets = (connectedIds.length > 1 ? connectedIds : [])
    .filter((id) => id !== voice?.serverId && serverStates.get(id) !== undefined)
    .map((id) => ({ serverId: id, label: nameOf(id) }));
  const message = stripMessage({
    error,
    connection: servers.connection.state,
    connected,
    voiceServerId: voice?.serverId ?? null,
    shownServerId,
  });
  // The subtitle under the name: the own game or playing track while connected (the game wins
  // the short line, it says more about being busy; the tooltip names both), else the server.
  const track = connected && ownTrack?.track.playing ? trackLabel(ownTrack.track) : null;
  const game = connected && ownGame ? ownGame.game.name : null;
  const subtitle =
    game ??
    track ??
    (connected || link !== null ? serverLabel || t("status.connected") : t("status.disconnected"));
  const subtitleTitle =
    game !== null && track !== null
      ? `${t("status.playing", { game })} · ${t("status.listening", { track })}`
      : subtitle;

  const openQuick = (x: number, y: number, opener: HTMLElement) => {
    setQuick({ x, y, opener });
  };

  // "Open quick settings" hotkey (#64): the menu opens at the gear button.
  useEffect(
    () =>
      onUiHotkey("openQuickSettings", () => {
        const button = settingsButton.current;
        if (button === null) return;
        const box = button.getBoundingClientRect();
        setQuick({ x: box.left, y: box.bottom, opener: button });
      }),
    [],
  );

  return (
    <div className="userpanel">
      <div className="userpanel__identity">
        {/* Plain text, not a control: profile edits live in the quick settings (gear). */}
        <div
          className={
            connected ? "userpanel__profile" : "userpanel__profile userpanel__profile--offline"
          }
        >
          {/* Muted or deafened also shows as a badge, talking as a ring: more than colour. */}
          <span
            className="userpanel__avatar"
            data-talking={transmitting ? "" : undefined}
            aria-hidden="true"
          >
            {/* The initial is drawn by CSS from data-initial, not as DOM text. */}
            <span
              className="userpanel-avatar"
              data-initial={avatarSrc ? undefined : name.slice(0, 1).toUpperCase()}
            >
              {avatarSrc ? <img className="userpanel-avatar__img" src={avatarSrc} alt="" /> : null}
            </span>
            {connected && (mute || deaf) ? (
              <span className="userpanel__badge">
                <Icon d={SLASH} />
              </span>
            ) : null}
          </span>
          <span className="userpanel__text">
            <span className="userpanel__name" data-tooltip={name} data-tooltip-overflow>
              {name}
            </span>{" "}
            <span className="userpanel__status" data-tooltip={subtitleTitle}>
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
              {subtitle}
            </span>
          </span>
        </div>
        <button
          ref={settingsButton}
          type="button"
          className="userpanel__toggle userpanel__toggle--settings"
          aria-label={t("settings")}
          data-tooltip={t("settings")}
          aria-haspopup="menu"
          onClick={(event) => {
            const box = event.currentTarget.getBoundingClientRect();
            openQuick(box.left, box.bottom, event.currentTarget);
          }}
          onContextMenu={(event) => {
            event.preventDefault();
            openQuick(event.clientX, event.clientY, event.currentTarget);
          }}
          onKeyDown={(event) => {
            if (event.key !== "ContextMenu" && !(event.key === "F10" && event.shiftKey)) return;
            event.preventDefault();
            const box = event.currentTarget.getBoundingClientRect();
            openQuick(box.left, box.bottom, event.currentTarget);
          }}
        >
          <Icon d={GEAR} />
        </button>
      </div>
      <StatusStrip
        message={message}
        error={error}
        serverLabel={serverLabel}
        voiceChannel={voice?.channelName ?? null}
        voiceServerLabel={nameOf(voice?.serverId ?? null)}
        joinChannel={shownSelf ? state.channelName : null}
        reconnect={
          servers.connection.state === "reconnecting"
            ? { attempt: servers.connection.attempt, max: servers.connection.maxAttempts }
            : null
        }
        connected={connected}
        onDismissError={() => {
          setError(null);
        }}
        onJoin={() => {
          if (shownSelf) run(joinChannel(shownSelf.channelId));
        }}
        onShowVoiceServer={() => {
          if (voice) showServer(voice.serverId);
        }}
      />
      <div className="userpanel__row">
        <span className="userpanel__split">
          <button
            ref={micButton}
            type="button"
            className="userpanel__toggle userpanel__toggle--mic"
            data-ptt-idle={pttIdle ? "" : undefined}
            aria-pressed={mute}
            aria-label={t("mute")}
            data-tooltip={micTitle}
            disabled={!connected}
            onClick={() => {
              run(getApi().setSelfMute(!mute));
            }}
            {...audio.triggers("input")}
          >
            {showLevel ? <span className="userpanel__level" aria-hidden="true" /> : null}
            <span className="userpanel__icon">
              <Icon d={MIC} />
              {mute || pttIdle ? <Icon d={SLASH} /> : null}
            </span>
          </button>
          <AudioArrow kind="input" audio={audio} disabled={!connected} />
        </span>
        <span className="userpanel__split">
          <button
            type="button"
            className="userpanel__toggle"
            aria-pressed={deaf}
            aria-label={t("deafen")}
            disabled={!connected}
            onClick={() => {
              run(getApi().setSelfDeaf(!deaf));
            }}
            {...audio.triggers("output")}
          >
            <span className="userpanel__icon">
              <Icon d={HEADPHONES} />
              {deaf ? <Icon d={SLASH} /> : null}
            </span>
          </button>
          <AudioArrow kind="output" audio={audio} disabled={!connected} />
        </span>
        {link !== null ? (
          <span className="userpanel__split">
            <button
              type="button"
              className="userpanel__toggle userpanel__toggle--hangup"
              aria-label={t("disconnect", { server: serverLabel })}
              data-tooltip={t("disconnect", { server: serverLabel })}
              aria-haspopup="menu"
              onClick={disconnectShown}
              onContextMenu={(event) => {
                event.preventDefault();
                openDisconnectMenu(event.clientX, event.clientY, event.currentTarget);
              }}
              onKeyDown={(event) => {
                if (event.key !== "ContextMenu" && !(event.key === "F10" && event.shiftKey)) return;
                event.preventDefault();
                openDisconnectMenuBelow(event.currentTarget);
              }}
            >
              <Icon d={HANG_UP} />
            </button>
            <button
              type="button"
              className="userpanel__arrow userpanel__toggle--more"
              aria-label={t("disconnectMenu.open")}
              data-tooltip={t("disconnectMenu.open")}
              aria-haspopup="menu"
              onClick={(event) => {
                openDisconnectMenuBelow(event.currentTarget);
              }}
            >
              <ArrowIcon />
            </button>
          </span>
        ) : (
          // Keeps the hang-up pill's place, so the other controls never shift.
          <span className="userpanel__split userpanel__split--empty" aria-hidden="true" />
        )}
      </div>
      {audio.node}
      {quick ? (
        <QuickSettings
          menu={quick}
          account={{
            connected: state.connected,
            registered: shownSelf?.registered ?? false,
            canRegister: state.canRegister,
            onEditProfile: () => {
              setDialog("profile");
            },
            onRegister: () => {
              setDialog("register");
            },
          }}
          onOpenSettings={openSettings}
          onClose={() => {
            setQuick(null);
          }}
        />
      ) : null}
      {disconnectMenu ? (
        <DisconnectMenu
          menu={disconnectMenu}
          serverLabel={serverLabel}
          canLeaveVoice={voice !== null && voice.serverId === link}
          switchTargets={switchTargets}
          connectedCount={connectedIds.length}
          onLeaveVoice={leaveVoice}
          onDisconnect={disconnectShown}
          onSwitchVoice={switchVoice}
          onDisconnectAll={() => {
            setConfirmAll(true);
          }}
          onClose={() => {
            setDisconnectMenu(null);
          }}
        />
      ) : null}
      {confirmAll ? (
        <ConfirmDialog
          title={t("disconnectAll.title")}
          message={t("disconnectAll.message", { total: connectedIds.length })}
          confirmLabel={t("disconnectAll.confirm")}
          danger
          onConfirm={() => {
            void disconnectAll();
          }}
          onClose={() => {
            setConfirmAll(false);
          }}
        />
      ) : null}
      {dialog === "profile" && shownSelf ? (
        <ProfileDialog
          session={shownSelf.session}
          hasAvatar={shownSelf.hasAvatar}
          registered={shownSelf.registered}
          onClose={closeDialog}
        />
      ) : null}
      {dialog === "register" ? <RegisterDialog onClose={closeDialog} /> : null}
    </div>
  );
}
