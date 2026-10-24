/**
 * What the pinned info box at the top of a channel's chat shows (FR-1.48b, QA.15): the channel
 * description, plus the server's welcome text in the root channel and in the default channel
 * when that channel has no description of its own.
 */
import type { Conversation } from "../../api";
import type { ServerState } from "../channels/serverStore";

export interface PinnedInfo {
  /** Server HTML blocks (untrusted, sanitize before rendering); empty means no box. */
  blocks: string[];
  /** True when the description was only announced by hash and must be requested. */
  needsFetch: boolean;
}

const NONE: PinnedInfo = { blocks: [], needsFetch: false };

function present(html: string | null | undefined): html is string {
  return typeof html === "string" && html.trim() !== "";
}

export function pinnedInfo(state: ServerState, conversation: Conversation): PinnedInfo {
  if (conversation.kind !== "channel") return NONE;
  const channel = state.channels.get(conversation.channelId);
  if (!channel) return NONE;
  const blocks: string[] = [];
  if (present(channel.description)) blocks.push(channel.description);
  const welcome = state.info?.welcomeText;
  const isRoot = channel.parentId === null;
  const isDefault = state.info?.defaultChannelId === channel.id;
  if (present(welcome) && (isRoot || (isDefault && !channel.hasDescription))) {
    blocks.push(welcome);
  }
  return { blocks, needsFetch: channel.hasDescription && channel.description === null };
}
