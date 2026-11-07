/**
 * "The relay of this server changed" as a notification (#600): an entry in the notification
 * centre, a toast or system notification like a message, and the details dialog with the old and
 * new fingerprint. Trusting stays a deliberate click; nothing here trusts by itself.
 */
import { useSyncExternalStore } from "react";
import type { TFunction } from "i18next";
import { getApi, type MumcordApi, type RelayInfo, type Unsubscribe } from "../../api";
import { relayItemId } from "../notifications/centreModel";
import { addItem, hasItem, removeItems } from "../notifications/centreStore";
import { showToast } from "../notifications/notificationStore";

/** The server whose relay details are open, or `null`. */
let detailsOf: number | null = null;
const listeners = new Set<() => void>();

function setDetails(serverId: number | null): void {
  detailsOf = serverId;
  for (const listener of listeners) listener();
}

export function openRelayDetails(serverId: number): void {
  setDetails(serverId);
}

export function closeRelayDetails(): void {
  setDetails(null);
}

export function getRelayDetails(): number | null {
  return detailsOf;
}

export function useRelayDetails(): number | null {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    () => detailsOf,
  );
}

/** Asks the engine to trust the announced relay; the centre entry goes once the change arrives. */
export async function trustNewRelay(serverId: number): Promise<void> {
  await getApi().relayTrustNew(serverId);
}

/** Waiting for the user's decision: a changed relay that was not kept blocked. */
export function needsDecision(info: RelayInfo): boolean {
  return info.status.kind === "untrusted" && !info.status.keptBlocked;
}

export interface RelayNoticeContext {
  t: TFunction<"p2p">;
  serverName: (serverId: number) => string | undefined;
}

async function announce(serverId: number, context: () => RelayNoticeContext): Promise<void> {
  const { t, serverName } = context();
  const server = serverName(serverId) ?? "";
  const title = t("trust.title", { server });
  const body = t("trust.body");
  let outcome;
  try {
    outcome = await getApi().notifyActivity({
      serverId,
      channelId: 0,
      summary: title,
      body,
      actionLabel: t("trust.details"),
    });
  } catch (error) {
    console.error("Deciding about the relay notification failed", error);
    outcome = "toast";
  }
  if (outcome !== "toast") return;
  showToast({
    key: relayItemId(serverId),
    serverId,
    conversation: { kind: "channel", channelId: 0 },
    title,
    text: body,
    open: () => {
      openRelayDetails(serverId);
    },
    actions: [
      {
        label: t("relay.trustNew"),
        primary: true,
        run: () => {
          trustNewRelay(serverId).catch((error: unknown) => {
            console.error("Trusting the new relay failed", error);
            openRelayDetails(serverId);
          });
        },
      },
      {
        label: t("trust.details"),
        run: () => {
          openRelayDetails(serverId);
        },
      },
    ],
  });
}

/**
 * Follows `relayChanged`: a relay that needs a decision gets one centre entry (and one
 * notification) per change; a decision removes the entry, and trusting confirms with a toast.
 */
export function bindRelayTrustNotices(
  api: MumcordApi,
  context: () => RelayNoticeContext,
): Unsubscribe {
  const waiting = new Set<number>();
  return api.on("relayChanged", ({ serverId, info }) => {
    const id = relayItemId(serverId);
    if (needsDecision(info)) {
      if (waiting.has(serverId) || hasItem(id)) return;
      waiting.add(serverId);
      addItem({ kind: "relay", id, serverId, at: Date.now(), read: false });
      void announce(serverId, context);
      return;
    }
    if (!waiting.delete(serverId) && !hasItem(id)) return;
    removeItems((item) => item.id === id);
    if (info.status.kind === "available") {
      const { t, serverName } = context();
      showToast({
        key: `${id}:trusted`,
        serverId,
        conversation: { kind: "channel", channelId: 0 },
        title: t("trust.trustedTitle"),
        text: t("trust.trustedBody", { server: serverName(serverId) ?? "" }),
        open: () => undefined,
      });
    }
  });
}
