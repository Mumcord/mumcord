/** What clicking an entry of the notification centre and its buttons do (#143, FR-3.106). */
import { getApi } from "../../api";
import { selectConversation, showServer } from "../channels";
import { openInBrowser } from "../chat/openLink";
import { revealMessage } from "../chat/reveal";
import { openRequestsView, selectRequest } from "../requests/useRequestsView";
import { startWatching } from "../screenshare";
import { openPluginItem } from "../plugins/notifications";
import { openUpdateDialog, requestUpdateRestart } from "../updates/dialogRequests";
import { setWatchParty } from "../watchparty/watchPartyStore";
import type { CentreItem, UpdateItem } from "./centreModel";
import { dismissItem, markRead, setCentreOpen } from "./centreStore";

/** Marks the entry read, closes the panel and shows its server and chat, message or request. */
export function openItem(item: CentreItem): void {
  markRead(item.id);
  if (item.kind === "plugin") {
    openPluginItem(item);
    return;
  }
  setCentreOpen(false);
  if (item.kind === "update") {
    openUpdateDialog();
    return;
  }
  showServer(item.serverId);
  switch (item.kind) {
    case "mention":
    case "reply":
      selectConversation(item.conversation);
      revealMessage(item.messageId);
      return;
    case "request":
      openRequestsView(item.serverId);
      selectRequest(item.request.id);
      return;
    case "invite":
      if (item.invite === "watchParty") {
        selectConversation({ kind: "channel", channelId: item.channelId });
      } else if (item.invite !== "screenShare") {
        selectConversation(item.conversation);
        revealMessage(item.messageId);
      }
      return;
    case "connection":
      return;
  }
}

/**
 * "Restart to update": the updater answers it like the toast's button, so a user in a call sees
 * the "ready" dialog first instead of being disconnected.
 */
export function restartForUpdate(itemId: string): void {
  markRead(itemId);
  setCentreOpen(false);
  requestUpdateRestart();
}

/**
 * "What's new": the release page with the changelog. The "ready" dialog lists no release notes,
 * so the page is where they are.
 */
export function showWhatsNew(item: UpdateItem): void {
  markRead(item.id);
  openInBrowser(item.url);
}

/** "Join" on a watch party invite: joins like the banner in the channel does (FR-2.39). */
export function joinWatchParty(serverId: number, itemId: string): void {
  getApi()
    .watchPartyJoin(serverId)
    .then((joined) => {
      setWatchParty(serverId, joined);
      dismissItem(itemId);
    })
    .catch((error: unknown) => {
      console.error("Joining the watch party failed", error);
    });
}

/** "Watch" on a screen share invite: opens the share like the banner does (FR-2.43). */
export function watchShare(serverId: number, session: number, itemId: string): void {
  showServer(serverId);
  startWatching(serverId, session);
  markRead(itemId);
  setCentreOpen(false);
}
