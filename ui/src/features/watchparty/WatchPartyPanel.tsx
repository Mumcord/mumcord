/**
 * The watch party above the chat of the user's channel (FR-2.39 – FR-2.41, spec 5.4). Users
 * who have not joined see a "Join watch party" button and nothing plays until they click it; participants see
 * the player, who did what last ("Anna paused", "Anna skipped"), the shared
 * playlist (FR-2.69); who is in the party and "Leave party" live in the widget title bar.
 */
import type { TFunction } from "i18next";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi, type User, type WatchCommand, type WatchParty } from "../../api";
import { useEmbedsAllowed } from "../../app/preferences";
import { useServerStateOf } from "../channels";
import { EmbedsOffNotice } from "./EmbedsOffNotice";
import { PlayPause } from "./PlayPause";
import { MediaPanel } from "../media/MediaPanel";
import { setTab, useMediaView } from "../media/mediaView";
import { useTick } from "../media/useTick";
import { WatchingList } from "./WatchingList";
import { PanelPlaylist } from "./PanelPlaylist";
import { Playlist } from "./Playlist";
import { playNext } from "./playlistActions";
import { WatchPartyPlayer } from "./WatchPartyPlayer";
import { joinWatchParty } from "./startParty";
import { useWatchParty } from "./watchPartyStore";
import "./watchParty.css";

function reportError(message: string, error: unknown): void {
  console.error(message, error);
  announce(message, "assertive");
}

/** The party of `serverId`, which need not be the shown server (the party follows the voice server). */
export function WatchPartyPanel({ serverId, channelId }: { serverId: number; channelId: number }) {
  const { t } = useTranslation("watchparty");
  const { t: tc } = useTranslation("common");
  const live = useWatchParty(serverId);
  const embeds = useEmbedsAllowed();
  const { users, selfSession } = useServerStateOf(serverId);
  const party = live?.party ?? null;
  const view = useMediaView();
  const nowMs = useTick(1000);

  const onCommand = useCallback(
    (command: WatchCommand) => {
      getApi()
        .watchPartyCommand(serverId, command)
        .catch((error: unknown) => {
          reportError(t("commandFailed"), error);
        });
    },
    [serverId, t],
  );
  const onEnded = useCallback(
    (entry: number) => {
      playNext(serverId, entry, false, t("playlist.nextFailed"));
    },
    [serverId, t],
  );

  if (live === null || party === null || party.channelId !== channelId) {
    return null;
  }

  const join = () => {
    joinWatchParty(serverId, t("joinFailed"));
  };

  if (!embeds) {
    return (
      <section className="watch-party" aria-label={t("title")}>
        <EmbedsOffNotice />
      </section>
    );
  }

  // With the cinema view open the panel is the right column's tab panel (issue #163).
  if (view.cinema === "watch" && party.joined) {
    const people = party.participants.flatMap((session) => {
      const user = users.get(session);
      return user ? [user] : [];
    });
    const watching = (
      <WatchingList
        serverId={serverId}
        live={live}
        users={users}
        selfSession={selfSession}
        nowMs={nowMs}
      />
    );
    return (
      <MediaPanel
        label={t("title")}
        tabs={[
          { id: "playlist", label: t("tabs.playlist"), count: party.playlist.length },
          { id: "watching", label: tc("media.watching"), count: people.length },
        ]}
        active={view.tab}
        onSelect={(id) => {
          if (id === "playlist" || id === "watching") setTab(id);
        }}
      >
        {view.tab === "watching" ? (
          watching
        ) : (
          <>
            <PanelPlaylist
              serverId={serverId}
              live={live}
              users={users}
              selfSession={selfSession}
            />
            <h3 className="media-panel__heading">{t("whoIsWatching")}</h3>
            {watching}
          </>
        )}
      </MediaPanel>
    );
  }

  return (
    <section className="watch-party" aria-label={t("title")}>
      <header className="watch-party__header">
        <h3 className="watch-party__title">
          {t("title")} <span className="watch-party__beta">{t("beta")}</span>
        </h3>
        {!party.joined && (
          <button type="button" className="button button--primary" onClick={join}>
            {t("join")}
          </button>
        )}
      </header>
      {party.joined && <WatchPartyPlayer live={live} onCommand={onCommand} onEnded={onEnded} />}
      <ActionNotice
        text={party.joined ? actionLine(t, party, users, selfSession) : ""}
        stamp={party.lastAction ?? party.skippedBy}
      />
      {party.joined && (
        <Playlist
          serverId={serverId}
          party={party}
          users={users}
          leading={<PlayPause serverId={serverId} live={live} />}
          compact
        />
      )}
    </section>
  );
}

/** Someone else's last action as a short notice; the user's own actions need none. */
function actionLine(
  t: TFunction<"watchparty">,
  party: WatchParty,
  users: ReadonlyMap<number, User>,
  selfSession: number | null,
): string {
  const nameOf = (session: number) => users.get(session)?.name ?? t("someone");
  if (party.skippedBy !== null) {
    return party.skippedBy === selfSession
      ? ""
      : t("other.skipped", { name: nameOf(party.skippedBy) });
  }
  const action = party.lastAction;
  if (action === null || action.session === selfSession) return "";
  return t(`other.${action.command.kind}`, { name: nameOf(action.session) });
}

/** How long a notice stays; the last part fades out (CSS), so keep both in step. */
export const NOTICE_MS = 4000;

/** Polite live region that shows `text` for {@link NOTICE_MS} each time `stamp` or `text` changes. */
function ActionNotice({ text, stamp }: { text: string; stamp: unknown }) {
  const [expired, setExpired] = useState<{ text: string; stamp: unknown } | null>(null);
  useEffect(() => {
    if (text === "") return;
    const timer = window.setTimeout(() => {
      setExpired({ text, stamp });
    }, NOTICE_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [text, stamp]);
  const visible = text !== "" && !(expired?.text === text && expired.stamp === stamp);
  return (
    <p className="watch-party__action" aria-live="polite">
      {visible && <span className="watch-party__notice">{text}</span>}
    </p>
  );
}
