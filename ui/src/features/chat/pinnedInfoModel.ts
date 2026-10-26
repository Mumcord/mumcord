/**
 * What the pinned info box at the top of a channel's chat shows (FR-1.48b, QA.15): the channel
 * description, plus the server's welcome text in the root channel and in the default channel
 * when that channel has no description of its own.
 */
import type { Conversation } from "../../api";
import type { ServerState } from "../channels/serverStore";
import { textHash } from "./hiddenInfo";

export interface PinnedInfo {
  /** Server HTML blocks (untrusted, sanitize before rendering); empty means no box. */
  blocks: string[];
  /** The welcome text shown in `blocks`, or null; the box offers a close button for it. */
  welcome: string | null;
  /** The channel description shown in `blocks`, or null; the box offers a close button for it. */
  description: string | null;
  /** True when the description was only announced by hash and must be requested. */
  needsFetch: boolean;
}

const NONE: PinnedInfo = {
  blocks: [],
  needsFetch: false,
  welcome: null,
  description: null,
};

function present(html: string | null | undefined): html is string {
  return typeof html === "string" && html.trim() !== "";
}

export function pinnedInfo(
  state: ServerState,
  conversation: Conversation,
  /** Fingerprints of the texts the user closed; each stays hidden while its text matches. */
  hidden: { welcome: string | null; description: string | null } = {
    welcome: null,
    description: null,
  },
): PinnedInfo {
  if (conversation.kind !== "channel") return NONE;
  const channel = state.channels.get(conversation.channelId);
  if (!channel) return NONE;
  const blocks: string[] = [];
  let shownDescription: string | null = null;
  if (present(channel.description) && textHash(channel.description) !== hidden.description) {
    blocks.push(channel.description);
    shownDescription = channel.description;
  }
  const welcome = state.info?.welcomeText;
  const isRoot = channel.parentId === null;
  const isDefault = state.info?.defaultChannelId === channel.id;
  let shownWelcome: string | null = null;
  if (
    present(welcome) &&
    textHash(welcome) !== hidden.welcome &&
    (isRoot || (isDefault && !channel.hasDescription))
  ) {
    blocks.push(welcome);
    shownWelcome = welcome;
  }
  return {
    blocks,
    welcome: shownWelcome,
    description: shownDescription,
    needsFetch: channel.hasDescription && channel.description === null,
  };
}
