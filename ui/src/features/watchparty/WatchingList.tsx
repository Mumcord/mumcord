/**
 * The watching list of the party panel (issue #163, FR-3.125): every person in the party with their
 * avatar, name, the host's crown and where they are – in sync, buffering, or N seconds behind. The
 * state is text as well as a colour, so it does not depend on colour alone.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import type { User } from "../../api";
import { Avatar } from "../members/Avatar";
import { useAvatars } from "../members/useAvatars";
import { syncState, type SyncState } from "./syncState";
import { positionAt, type LiveParty } from "./watchPartyStore";

export function WatchingList({
  serverId,
  live,
  users,
  selfSession,
  nowMs,
}: {
  serverId: number;
  live: LiveParty;
  users: ReadonlyMap<number, User>;
  selfSession: number | null;
  nowMs: number;
}): ReactNode {
  const { t } = useTranslation("common");
  const party = live.party;
  const people: User[] = party.participants.flatMap((session) => {
    const user = users.get(session);
    return user ? [user] : [];
  });
  const avatarOf = useAvatars(people, serverId);
  const positionMs = positionAt(live, nowMs);

  return (
    <ul className="watch-list">
      {people.map((person) => {
        const report = party.positions.find((p) => p.session === person.session);
        const state =
          report === undefined ? { kind: "unknown" as const } : syncState(report, positionMs, live.atMs, nowMs);
        return (
          <li key={person.session} className="watch-list__person">
            <Avatar name={person.name} src={avatarOf(person)} size="sm" />
            <span className="watch-list__name">
              {person.name}
              {person.session === selfSession && ` (${t("media.you")})`}
            </span>
            {person.session === party.host && (
              <span className="watch-list__host" data-tooltip={t("media.host")}>
                <span aria-hidden="true">♔</span>
                <span className="visually-hidden">{t("media.host")}</span>
              </span>
            )}
            <span className={`watch-list__state watch-list__state--${state.kind}`}>{stateLabel(t, state)}</span>
          </li>
        );
      })}
    </ul>
  );
}

function stateLabel(t: TFunction<"common">, state: SyncState): string {
  switch (state.kind) {
    case "synced":
      return t("media.inSync");
    case "buffering":
      return t("media.buffering");
    case "behind":
      return t("media.behind", { seconds: String(state.seconds) });
    case "ahead":
      return t("media.ahead", { seconds: String(state.seconds) });
    default:
      return t("media.noReport");
  }
}
