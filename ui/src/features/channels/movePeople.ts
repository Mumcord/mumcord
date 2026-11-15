/**
 * Moving people to another channel (Epic 4, issue #255): the one open channel picker behind the
 * person menu's "Move to…" and the channel menu's "Move everyone to…", and the paced send with
 * the result afterwards (FR-10.17, FR-10.18). `MovePeopleDialogHost` shows the picker once for the
 * whole window.
 */
import type { TFunction } from "i18next";
import { getApi } from "../../api";
import { noticeKey, showToast } from "../notifications/notificationStore";
import { getServerStateOf } from "./serverStore";

export interface MovePeopleRequest {
  /** The people to move. */
  sessions: readonly number[];
  /**
   * The channel all of them sit in, when they share one; `null` when they are spread over several,
   * so the picker leaves out a source that would name only one of them.
   */
  fromChannelId: number | null;
}

let current: MovePeopleRequest | null = null;
const listeners = new Set<() => void>();

function set(next: MovePeopleRequest | null): void {
  current = next;
  for (const listener of listeners) listener();
}

export function subscribeMovePeople(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function currentMovePeople(): MovePeopleRequest | null {
  return current;
}

/** Opens the picker (replacing any open one). */
export function openMovePeople(request: MovePeopleRequest): void {
  set(request);
}

export function closeMovePeople(): void {
  if (current !== null) set(null);
}

/**
 * How long Mumcord waits for people to arrive before it calls them refused (FR-10.18). The server
 * moves people asynchronously, so a move that was accepted still needs its `UserState` echo.
 */
export const MOVE_ARRIVAL_GRACE_MS = 5000;

/** Test hook: a shorter wait, so a test does not sit out the grace period. */
let arrivalGraceMs = MOVE_ARRIVAL_GRACE_MS;

export function setMoveArrivalGrace(ms: number): void {
  arrivalGraceMs = ms;
}

export interface MoveResult {
  /** People the move was meant for. */
  total: number;
  /** People who sat in the target channel when the grace period was over. */
  moved: number;
  /** Moves the engine itself refused, e.g. for a person who left meanwhile. */
  rejected: number;
}

/** Servers with a mass move in flight, so a second one cannot overtake the first (FR-10.18). */
const running = new Set<number>();

/** True while a mass move on this server is being sent and counted. */
export function massMoveRunning(serverId: number): boolean {
  return running.has(serverId);
}

/**
 * Sends one move per person and counts who arrived. The server paces the messages itself (see
 * `ratelimit.rs` of the `mumble-client-protocol` crate), so a whole channel can be sent in one
 * go (FR-10.18).
 */
export async function movePeopleToChannel(
  serverId: number,
  sessions: readonly number[],
  channelId: number,
): Promise<MoveResult> {
  running.add(serverId);
  try {
    const results = await Promise.allSettled(
      sessions.map((session) => getApi().personMove(serverId, session, channelId)),
    );
    const rejected = results.filter((result) => result.status === "rejected");
    for (const result of rejected) {
      console.error("Moving a person failed", result.reason);
    }
    // Who arrived is read from the server's own state, never assumed (FR-10.18).
    await new Promise((resolve) => {
      setTimeout(resolve, arrivalGraceMs);
    });
    const users = getServerStateOf(serverId).users;
    const moved = sessions.filter((session) => users.get(session)?.channelId === channelId).length;
    return { total: sessions.length, moved, rejected: rejected.length };
  } finally {
    running.delete(serverId);
  }
}

/**
 * One notice for a whole group: who arrived and that the rest were refused (FR-10.17). Single
 * people get no extra line: the tree already shows where they are (FR-10.59).
 */
export function showMoveResult(
  t: TFunction<"channels">,
  serverId: number,
  channelId: number,
  result: MoveResult,
): void {
  if (result.total < 2) return;
  const channel = getServerStateOf(serverId).channels.get(channelId)?.name ?? "";
  const people = t("tree.people", { count: result.total });
  const complete = result.moved === result.total;
  showToast({
    key: noticeKey(serverId, `move:${String(channelId)}`),
    serverId,
    conversation: { kind: "channel", channelId },
    title: t("moveTo.resultTitle"),
    text: complete
      ? t("moveTo.resultAll", { people, channel })
      : t("moveTo.resultSome", { moved: String(result.moved), people, channel }),
    open: () => undefined,
  });
}

/** A second mass move on the same server waits: the first one is still counting (FR-10.18). */
export function showMoveAlreadyRunning(
  t: TFunction<"channels">,
  serverId: number,
  channelId: number,
): void {
  showToast({
    key: noticeKey(serverId, "move-running"),
    serverId,
    conversation: { kind: "channel", channelId },
    title: t("moveTo.resultTitle"),
    text: t("moveTo.busy"),
    open: () => undefined,
  });
}
