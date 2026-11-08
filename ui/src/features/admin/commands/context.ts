/**
 * Where an admin command runs (Epic 4, issue #378): one server, the chat its text was typed in, and
 * the words for the lines it leaves behind. Commands, their confirmations and the composer hook all
 * work with the same context, so a result line always lands in the chat the admin is looking at
 * (FR-10.35, Q4.6).
 */
import type { TFunction } from "i18next";
import type { Conversation } from "../../../api";
import type { ServerState } from "../../channels/serverStore";
import { report } from "./lines";

export interface CommandContext {
  serverId: number;
  conversation: Conversation;
  state: ServerState;
  t: TFunction<"admin">;
}

/** The private line of one command, with the note that only its admin sees it (FR-10.61). */
export function say(ctx: CommandContext, text: string): void {
  report({
    serverId: ctx.serverId,
    conversation: ctx.conversation,
    text,
    note: ctx.t("commands.onlyYou"),
  });
}
