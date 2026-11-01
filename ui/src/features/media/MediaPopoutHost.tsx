/**
 * Keeps the pop-out windows (issue #164) in step with the main window, which owns the party, the
 * watch and the connection: a pop-out closes when what it shows ends, its window title follows the
 * video and the channel, and a view comes back into the main column when its window closes.
 * Renders nothing; mounted once in the main window.
 */
import { useEffect, useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type MediaPopoutKind, type MediaPopoutTarget } from "../../api";
import { useServerStateOf } from "../channels";
import { useWatchedShare, watchedShare } from "../screenshare/watching";
import { useWatchParty, watchPartyOf } from "../watchparty/watchPartyStore";
import { openMedia } from "./mediaView";
import { popIn, sameTarget, usePopouts, usePopoutTitle } from "./popoutStore";

/** Whether the main window still has what the closed pop-out showed. */
function stillActive(kind: MediaPopoutKind, target: MediaPopoutTarget): boolean {
  if (kind === "watch") return watchPartyOf(target.serverId)?.party.joined ?? false;
  return sameTarget(watchedShare(), target);
}

export function MediaPopoutHost(): ReactNode {
  const popouts = usePopouts();
  const previous = useRef(popouts);

  useEffect(() => {
    const before = previous.current;
    previous.current = popouts;
    for (const kind of ["watch", "share"] as const) {
      const was = before[kind].target;
      if (was !== null && popouts[kind].target === null && stillActive(kind, was)) openMedia(kind);
    }
  }, [popouts]);

  const watch = popouts.watch.target;
  const share = popouts.share.target;
  return (
    <>
      {watch !== null && <WatchPopoutSync serverId={watch.serverId} />}
      {share !== null && share.session !== null && (
        <SharePopoutSync serverId={share.serverId} session={share.session} />
      )}
    </>
  );
}

/** Renames the open window when its title changes. */
function useWindowTitle(kind: MediaPopoutKind, title: string): void {
  const opened = useRef(title);
  useEffect(() => {
    if (opened.current === title) return;
    opened.current = title;
    getApi()
      .mediaPopoutSetTitle(kind, title)
      .catch((error: unknown) => {
        console.error("Renaming the pop-out window failed", error);
      });
  }, [kind, title]);
}

function WatchPopoutSync({ serverId }: { serverId: number }): null {
  const { t } = useTranslation("watchparty");
  const { t: tc } = useTranslation("common");
  const joined = useWatchParty(serverId)?.party.joined ?? false;
  useWindowTitle("watch", usePopoutTitle(serverId, t("title")));
  useEffect(() => {
    if (!joined) popIn("watch", tc("media.popInFailed"));
  }, [joined, tc]);
  return null;
}

function SharePopoutSync({ serverId, session }: { serverId: number; session: number }): null {
  const { t } = useTranslation("screenviewer");
  const { t: tc } = useTranslation("common");
  const watched = useWatchedShare();
  const { users } = useServerStateOf(serverId);
  const name = users.get(session)?.name ?? t("someone");
  useWindowTitle("share", usePopoutTitle(serverId, t("title", { name })));
  const watching = sameTarget(watched, { serverId, session });
  useEffect(() => {
    if (!watching) popIn("share", tc("media.popInFailed"));
  }, [watching, tc]);
  return null;
}
