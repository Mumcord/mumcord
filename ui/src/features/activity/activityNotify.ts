/**
 * Activity notifications (#597): a new event in the activity feed notifies like a message, as an
 * in-app toast or a system notification. The engine applies Do Not Disturb, a muted server, the
 * window focus and the mode of the event's kind (#660); a kind set to Off stops here already. Own events, blocked people's events, events from before this connection and "stopped" events never notify.
 */
import type { TFunction } from "i18next";
import { getApi, type ActivityNotifySettings } from "../../api";
import { getServerStateOf } from "../channels";
import { showToast } from "../notifications/notificationStore";
import type { ActivityEvent } from "./activityStore";

export interface ActivityNoticeContext {
  t: TFunction<"activity">;
  /** The label of the "open" action, from the message notifications. */
  openLabel: string;
  serverName: (serverId: number) => string | undefined;
}

/**
 * One join, leave or move notification per person in this time. Hopping channels or reconnecting
 * quickly would otherwise flood the toasts and system notifications; later events in the interval
 * still reach the history.
 */
export const ACTIVITY_NOTICE_INTERVAL_MS = 30_000;

const lastNoticeAt = new Map<string, number>();

export function resetActivityThrottle(): void {
  lastNoticeAt.clear();
}

/** True when this person was already announced inside the interval; otherwise records it. */
function throttled(event: ActivityEvent): boolean {
  if (event.kind === "pluginEvent") return false;
  const key = `${String(event.serverId)}:${event.name ?? String(event.session ?? "")}`;
  const last = lastNoticeAt.get(key);
  if (last !== undefined && event.at - last < ACTIVITY_NOTICE_INTERVAL_MS) return true;
  lastNoticeAt.set(key, event.at);
  return false;
}

/** The switch that decides about an event, or `null` when the event never notifies. */
export function switchOf(event: ActivityEvent): keyof ActivityNotifySettings | null {
  if (event.earlier === true) return null;
  if (event.kind !== "pluginEvent" && event.blocked === true) return null;
  switch (event.kind) {
    case "connected":
    case "disconnected":
    case "moved":
      return event.self ? null : "joins";
    case "screenShare":
    case "watchParty":
    case "pluginActivity":
      return event.self || !event.started ? null : event.kind;
    case "pluginEvent":
      return "pluginEvent";
  }
}

function actionOf(event: ActivityEvent, t: TFunction<"activity">): string {
  switch (event.kind) {
    case "connected":
    case "disconnected":
    case "moved":
      return t(`verb.${event.kind}`);
    case "screenShare":
      return t("verb.screenShareStarted");
    case "watchParty":
      return t("verb.watchPartyStarted");
    case "pluginActivity":
      return t("verb.pluginActivityStarted", { plugin: event.pluginName });
    case "pluginEvent":
      return t("verb.pluginEvent", { text: event.plugin.text });
  }
}

/** The plain body line, e.g. "Ben started a screen share in #Lobby". */
export function noticeBody(event: ActivityEvent, t: TFunction<"activity">): string {
  const action = actionOf(event, t);
  if (event.kind === "pluginEvent") return action;
  const name = event.name ?? t("someone");
  const channel = "channelName" in event ? event.channelName : null;
  return channel === null
    ? t("notice.body", { name, action })
    : t("notice.bodyInChannel", { name, action, channel });
}

/** The channel whose chat a click opens: where the event happened, else the root channel. */
function channelIdOf(event: ActivityEvent): number {
  const name = "channelName" in event ? event.channelName : null;
  if (name === null) return 0;
  for (const channel of getServerStateOf(event.serverId).channels.values()) {
    if (channel.name === name) return channel.id;
  }
  return 0;
}

async function switches(): Promise<ActivityNotifySettings | null> {
  try {
    return (await getApi().settingsGet()).activityNotify;
  } catch (error) {
    console.error("Reading the activity notification settings failed", error);
    return null;
  }
}

export async function handleActivity(
  event: ActivityEvent,
  context: ActivityNoticeContext,
): Promise<void> {
  const kind = switchOf(event);
  if (kind === null) return;
  const modes = await switches();
  if (modes === null || modes[kind] === "off") return;
  if (kind === "joins" && throttled(event)) return;
  const summary = context.serverName(event.serverId) ?? "";
  const body = noticeBody(event, context.t);
  const channelId = channelIdOf(event);
  let outcome;
  try {
    outcome = await getApi().notifyActivity({
      kind,
      serverId: event.serverId,
      channelId,
      summary,
      body,
      actionLabel: context.openLabel,
    });
  } catch (error) {
    console.error("Deciding about an activity notification failed", error);
    outcome = "toast";
  }
  if (outcome !== "toast") return;
  showToast({
    key: `activity:${String(event.serverId)}:${String(event.at)}`,
    serverId: event.serverId,
    conversation: { kind: "channel", channelId },
    title: summary,
    text: body,
  });
}
