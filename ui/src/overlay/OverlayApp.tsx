/**
 * Overlay window content (FR-1.44): who is talking, or the whole own channel. Reads the shared
 * server store (same engine events as the main window) through narrow selectors, so it renders
 * only when talking state or the shown people change.
 */
import { useCallback, useEffect } from "react";
import { getApi, toEngineError, type User } from "../api";
import { sameValue } from "../features/channels/cachedSelector";
import { selfUser, useServerSelector, type ServerState } from "../features/channels/serverStore";
import { useAvatars } from "../features/members/useAvatars";
import { OverlayView, type OverlayPerson } from "./OverlayView";
import { useOverlayState } from "./useOverlayState";
import { useTalkers } from "./useTalkers";

const selectChannelName = (s: ServerState): string | null => {
  const me = s.connected ? selfUser(s) : undefined;
  return me === undefined ? null : (s.channels.get(me.channelId)?.name ?? null);
};

const selectTalkingSet = (s: ServerState) => s.talking;

function leaveMoveMode(): void {
  getApi()
    .overlayMoveMode(false)
    .catch((cause: unknown) => {
      console.error("Leaving the overlay move mode failed", toEngineError(cause).code);
    });
}

export function OverlayApp() {
  const overlay = useOverlayState();
  const wholeChannel = overlay?.settings.showWholeChannel ?? false;
  const talkers = useTalkers();
  const talking = useServerSelector(selectTalkingSet);
  const channelName = useServerSelector(selectChannelName);

  const talkerKey = talkers.map((t) => t.session).join(",");
  const selectPeople = useCallback(
    (s: ServerState): readonly User[] => {
      if (!s.connected) return [];
      if (wholeChannel) {
        const me = selfUser(s);
        if (me === undefined) return [];
        return [...s.users.values()]
          .filter((u) => u.channelId === me.channelId)
          .sort((a, b) => a.name.localeCompare(b.name));
      }
      const sessions = talkerKey === "" ? [] : talkerKey.split(",").map(Number);
      return sessions.flatMap((session) => s.users.get(session) ?? []);
    },
    [wholeChannel, talkerKey],
  );
  const users = useServerSelector(selectPeople, sameValue);
  const avatarOf = useAvatars(users);

  const moveMode = overlay?.moveMode ?? false;
  useEffect(() => {
    if (!moveMode) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") leaveMoveMode();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [moveMode]);

  if (overlay === null) return null;
  const fading = new Set(talkers.filter((t) => t.fading).map((t) => t.session));
  const people: OverlayPerson[] = users.map((user) => ({
    session: user.session,
    name: user.name,
    avatar: avatarOf(user),
    talking: talking.has(user.session),
    fading: !wholeChannel && fading.has(user.session),
  }));
  return (
    <OverlayView
      people={people}
      channelName={overlay.settings.showChannelName ? channelName : null}
      opacity={overlay.settings.opacity / 100}
      moveMode={moveMode}
      onDone={leaveMoveMode}
    />
  );
}
