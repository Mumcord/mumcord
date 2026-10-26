/**
 * Avatar images for a list of people, loaded lazily via `person_profile` and cached per session.
 * Only people with `hasAvatar` are fetched; everyone else gets `null` (initials fallback). An
 * entry is dropped when `hasAvatar` or `avatarHash` changes, the person leaves, or the connection is re-synced or
 * lost (sessions are reused across connections). An answer without an image (blob not received in
 * time, person not found) is fetched again on the person's next change, as the texture blob can
 * arrive later under the same hash.
 *
 * Answers are applied once per animation frame, so a burst of them re-renders once.
 *
 * Usage: `const avatarOf = useAvatars(visibleUsers); <Avatar src={avatarOf(user)} … />`.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { getApi, toEngineError, type User } from "../../api";
import { shownServerId, subscribeServerChanges } from "../channels/serverStore";

export function useAvatars(
  users: readonly User[],
  /** The server the users belong to; the shown one when omitted. */
  serverId?: number,
): (user: User) => string | null {
  const [images, setImages] = useState<ReadonlyMap<number, string | null>>(() => new Map());
  /** Per-session generation; bumped on invalidation so late responses are discarded. */
  const generations = useRef(new Map<number, number>());
  /** Sessions with a request in flight or a cached result for the current generation. */
  const requested = useRef(new Set<number>());
  /** Sessions whose current answer has no image; any change of the person retries them. */
  const empty = useRef(new Set<number>());
  /** Sessions already reported as not found, so a retry does not log again. */
  const reportedMissing = useRef(new Set<number>());
  /** Answers waiting for the next frame: hundreds can arrive at once on a big server. */
  const arrived = useRef(new Map<number, { src: string | null; generation: number }>());
  const flushFrame = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (flushFrame.current !== null) cancelAnimationFrame(flushFrame.current);
    },
    [],
  );

  useEffect(() => {
    const forget = (sessions: Iterable<number> | "all") => {
      const list = sessions === "all" ? [...requested.current] : [...sessions];
      for (const session of list) {
        requested.current.delete(session);
        empty.current.delete(session);
        generations.current.set(session, (generations.current.get(session) ?? 0) + 1);
      }
      if (list.length === 0) return;
      // Always a new map, so the fetch effect re-runs even for requests that were in flight.
      setImages((current) => {
        const next = new Map(current);
        for (const session of list) next.delete(session);
        return next;
      });
    };
    return subscribeServerChanges((change) => {
      if (change.kind === "synced" || change.kind === "connection" || change.kind === "shown")
        forget("all");
      else if (change.kind === "userRemoved") forget([change.user.session]);
      else if (
        change.kind === "userUpserted" &&
        (empty.current.has(change.user.session) ||
          (change.previous !== undefined &&
            (change.previous.hasAvatar !== change.user.hasAvatar ||
              change.previous.avatarHash !== change.user.avatarHash)))
      ) {
        forget([change.user.session]);
      }
    });
  }, []);

  const wanted = users
    .filter((u) => u.hasAvatar)
    .map((u) => u.session)
    .sort((a, b) => a - b)
    .join(",");

  // `images` is a dependency so invalidated sessions (removed from it) are fetched again.
  useEffect(() => {
    for (const session of wanted === "" ? [] : wanted.split(",").map(Number)) {
      if (requested.current.has(session)) continue;
      requested.current.add(session);
      const generation = generations.current.get(session) ?? 0;
      const store = (src: string | null) => {
        if ((generations.current.get(session) ?? 0) !== generation) return;
        if (src === null) empty.current.add(session);
        else empty.current.delete(session);
        arrived.current.set(session, { src, generation });
        flushFrame.current ??= requestAnimationFrame(() => {
          flushFrame.current = null;
          const batch = arrived.current;
          arrived.current = new Map();
          // A session invalidated while its answer waited is fetched again; drop the stale one.
          setImages((current) => {
            const next = new Map(current);
            for (const [s, answer] of batch) {
              if ((generations.current.get(s) ?? 0) === answer.generation) next.set(s, answer.src);
            }
            return next;
          });
        });
      };
      getApi()
        .personProfile(serverId ?? shownServerId(), session)
        .then((profile) => {
          store(profile.avatar);
        })
        .catch((cause: unknown) => {
          const { code } = toEngineError(cause);
          // The person left before the answer came: expected, and retried if they change again.
          if (code !== "notFound") console.error("Loading an avatar failed", code);
          else if (!reportedMissing.current.has(session)) {
            reportedMissing.current.add(session);
            console.debug("Avatar of a person who is gone was not loaded");
          }
          store(null);
        });
    }
  }, [wanted, images, serverId]);

  return useCallback(
    (user: User) => (user.hasAvatar ? (images.get(user.session) ?? null) : null),
    [images],
  );
}
