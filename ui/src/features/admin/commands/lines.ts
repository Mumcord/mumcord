/**
 * The line a finished admin command leaves in the chat (Epic 4, issue #378, FR-10.35): what the
 * server did, in words, and the note that only this admin sees it. Both the commands that act at
 * once and the ones that wait for a confirmation end here.
 */
import type { Conversation } from "../../../api";
import { addPrivateLine } from "../../chat/privateLines";
import { announce } from "../../../a11y";

/** One line, already translated: the command's own result and the note beside it. */
export interface ReportInput {
  serverId: number;
  conversation: Conversation;
  text: string;
  note: string;
}

/** The line in the chat, read out to the screen reader too (spec 10, section 5.8). */
export function report(input: ReportInput): void {
  addPrivateLine({
    serverId: input.serverId,
    conversation: input.conversation,
    text: input.text,
    note: input.note,
  });
  announce(input.text);
}
