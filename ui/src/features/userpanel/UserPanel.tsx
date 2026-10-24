import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { useView } from "../../app/view";
import { getApi } from "../../api";
import { ConnectionInfo } from "../connection";
import { disconnect, useServers } from "../servers";
import { useAvatars } from "../members/useAvatars";
import { describeError } from "./errors";
import { ProfileDialog } from "./ProfileDialog";
import { joinChannel, useReadOnly } from "./readOnly";
import { RegisterDialog } from "./RegisterDialog";
import { useMicLevelFill } from "./useMicLevelFill";
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
const HEADPHONES = "M4 15v-3a8 8 0 0 1 16 0v3M4 15h3v5H4zM17 15h3v5h-3z";
const GEAR =
  "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2";
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
  const showLevel = state.connected && !mute && !deaf;
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
            : serverLabel || t("status.connected");

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
            <span className="userpanel__status" title={status}>
              {status}
            </span>
          </span>
        </button>
      </div>
      <div className="userpanel__row">
        <button
          ref={micButton}
          type="button"
          className="userpanel__toggle userpanel__toggle--mic"
          aria-pressed={mute}
          aria-label={t("mute")}
          disabled={!connected}
          onClick={() => {
            run(getApi().setSelfMute(!mute));
          }}
        >
          {showLevel ? <span className="userpanel__level" aria-hidden="true" /> : null}
          <span className="userpanel__icon">
            <Icon d={MIC} />
            {mute ? <Icon d={SLASH} /> : null}
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
        >
          <Icon d={GEAR} />
        </button>
      </div>
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
