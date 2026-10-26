import { useCallback, useRef, useState, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { EngineError, getApi } from "../../api";
import { shownServerId } from "../channels/serverStore";
import { useServerState } from "../channels";
import { useServers } from "../servers";
import { displayName } from "../servers/format";
import { ServerMenu } from "../servers/ServerMenu";
import { describeFollowError } from "./errors";
import { setFollowNotice, useFollowNotice } from "./notice";
import { describeSummary } from "./summary";

function describeFailure(t: TFunction<"follow">, error: unknown, action: Action): string {
  if (error instanceof EngineError && ["serverTooOld", "notConnected"].includes(error.code)) {
    return describeFollowError(t, error);
  }
  console.error(`${action === "follow" ? "Following" : "Unfollowing"} all channels failed`, error);
  return action === "follow" ? t("all.failed") : t("all.unfollowFailed");
}

type Action = "follow" | "unfollow";

/**
 * Server name row above the channel tree with a "⋯" menu (also on right-click): "Follow all
 * channels" and "Unfollow all channels" (QA.13). Hidden on servers without follow support.
 */
export function FollowAllMenu() {
  const { t } = useTranslation("follow");
  const server = useServerState();
  const { servers } = useServers();
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [menuAt, setMenuAt] = useState<{ x: number; y: number } | null>(null);
  const [busy, setBusy] = useState(false);

  const close = useCallback((returnFocus: boolean) => {
    setMenuAt(null);
    if (returnFocus) buttonRef.current?.focus();
  }, []);

  if (!server.info?.followSupported) return null;
  const saved = servers.find((s) => s.id === server.serverId);
  const name = saved ? displayName(saved) : "";

  async function run(action: Action) {
    setMenuAt(null);
    buttonRef.current?.focus();
    setBusy(true);
    try {
      if (action === "follow") {
        setFollowNotice({ text: t("all.working"), error: false });
        const summary = await getApi().channelFollowAll(shownServerId());
        setFollowNotice({ text: describeSummary(t, summary), error: false });
      } else {
        setFollowNotice(null);
        await getApi().channelUnfollowAll(shownServerId());
        setFollowNotice({ text: t("all.unfollowed"), error: false });
      }
    } catch (e) {
      setFollowNotice({ text: describeFailure(t, e, action), error: true });
    } finally {
      setBusy(false);
    }
  }

  const openAtButton = () => {
    const rect = buttonRef.current?.getBoundingClientRect();
    setMenuAt({ x: rect?.left ?? 0, y: rect?.bottom ?? 0 });
  };

  return (
    // Right-click is a mouse shortcut; the "⋯" button is the keyboard route to the same menu.
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <div
      className="follow-server"
      onContextMenu={(event: MouseEvent) => {
        event.preventDefault();
        setMenuAt({ x: event.clientX, y: event.clientY });
      }}
    >
      <span className="follow-server__name">{name}</span>
      <button
        ref={buttonRef}
        type="button"
        className="follow-server__menu"
        aria-label={name ? t("all.menu", { server: name }) : t("all.menuLabel")}
        aria-haspopup="menu"
        aria-expanded={menuAt !== null}
        aria-disabled={busy}
        onClick={() => {
          if (busy) return;
          if (menuAt) setMenuAt(null);
          else openAtButton();
        }}
      >
        {/* An SVG rather than a "⋯" glyph: the button's name is its translated aria-label, and a
            text glyph would leak into the UI text as an untranslated string. */}
        <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" focusable="false">
          <circle cx="5" cy="12" r="1.75" fill="currentColor" />
          <circle cx="12" cy="12" r="1.75" fill="currentColor" />
          <circle cx="19" cy="12" r="1.75" fill="currentColor" />
        </svg>
      </button>
      {menuAt && !busy && (
        <ServerMenu
          label={t("all.menuLabel")}
          position={menuAt}
          onClose={close}
          items={[
            {
              id: "follow-all",
              label: t("all.follow"),
              onSelect: () => {
                void run("follow");
              },
            },
            {
              id: "unfollow-all",
              label: t("all.unfollow"),
              onSelect: () => {
                void run("unfollow");
              },
            },
          ]}
        />
      )}
    </div>
  );
}

/** Message line below the channel tree for the star and "Follow all". */
export function FollowNoticeLine() {
  const { t } = useTranslation("follow");
  const notice = useFollowNotice();
  const { serverId } = useServerState();
  return (
    // Always mounted, so screen readers pick up changes of the live region.
    <div className="follow-notice" role="status">
      {notice && serverId !== null && (
        <p className={notice.error ? "follow-error" : "follow-hint"}>
          {notice.text}{" "}
          <button
            type="button"
            className="follow-notice__dismiss"
            onClick={() => {
              setFollowNotice(null);
            }}
          >
            {t("all.dismiss")}
          </button>
        </p>
      )}
    </div>
  );
}
