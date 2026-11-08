/**
 * The two admin commands that ask first (Epic 4, issue #378, FR-10.55): `/unban` and `/moveall`.
 * What a command found is held here for the one open question, and dropped when it is answered or
 * the box changes. Nothing of it is stored (FR-10.63).
 */
import { useSyncExternalStore } from "react";
import { getApi, toEngineError, type BanEntry, type Conversation } from "../../../api";
import { actionErrorText } from "../errors";
import { say, type CommandContext } from "./context";

export type PendingConfirm =
  /** One entry of the server's ban list, shown with its reason and end (FR-10.51). */
  | { kind: "unban"; serverId: number; conversation: Conversation; entry: BanEntry }
  /** Everyone in the admin's channel moves; the count is in the question (FR-10.17). */
  | {
      kind: "moveAll";
      serverId: number;
      conversation: Conversation;
      fromId: number;
      from: string;
      toId: number;
      to: string;
      count: number;
    };

let current: PendingConfirm | null = null;
const listeners = new Set<() => void>();

function set(next: PendingConfirm | null): void {
  current = next;
  for (const listener of listeners) listener();
}

/** Tests start with no question open. */
export function resetConfirm(): void {
  set(null);
}

export function stageConfirm(next: PendingConfirm): void {
  set(next);
}

export function dismissConfirm(): void {
  set(null);
}

export function useAdminConfirm(): PendingConfirm | null {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => current,
  );
}

/**
 * The answer to "Unban Mallory?": the entry is removed from the server's list with the safe save
 * of FR-10.66, and only then does the list change. A refusal or a changed entry is reported as a
 * line in the chat, because the dialog is closed by then (FR-10.59).
 */
export async function confirmUnban(ctx: CommandContext): Promise<void> {
  const confirm = current;
  if (confirm === null || confirm.kind !== "unban") return;
  const api = getApi();
  const name = confirm.entry.name ?? confirm.entry.address;
  try {
    const outcome = await api.banListSave(
      ctx.serverId,
      [{ kind: "remove", key: confirm.entry.key }],
      false,
    );
    if (outcome.kind === "conflict") {
      say(ctx, ctx.t("commands.errors.banChanged"));
      return;
    }
    say(ctx, ctx.t("commands.results.unbanned", { name }));
    set(null);
  } catch (error) {
    say(ctx, actionErrorText(ctx.t, toEngineError(error)));
    set(null);
  }
}

/**
 * The answer to "Move 6 people from General to AFK?": the engine sends the moves at the paced rate
 * and counts who arrived (FR-10.17, FR-10.18); the question is closed while they are on their way,
 * because the same command may not run twice at once.
 */
export async function confirmMoveAll(ctx: CommandContext): Promise<void> {
  const confirm = current;
  if (confirm === null || confirm.kind !== "moveAll") return;
  set(null);
  const api = getApi();
  try {
    const outcome = await api.personMoveAll(ctx.serverId, confirm.toId);
    if (outcome.kind === "alreadyRunning") {
      say(ctx, ctx.t("commands.results.alreadyMoving"));
      return;
    }
    say(
      ctx,
      outcome.moved === outcome.total
        ? ctx.t("commands.results.movedAll", { moved: outcome.moved, total: outcome.total })
        : ctx.t("commands.results.movedSome", {
            moved: outcome.moved,
            total: outcome.total,
            refused: outcome.total - outcome.moved,
          }),
    );
  } catch (error) {
    say(ctx, actionErrorText(ctx.t, toEngineError(error)));
  }
}
