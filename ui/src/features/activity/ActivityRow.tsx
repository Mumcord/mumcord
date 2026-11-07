/**
 * One row of the feed (FR-11.11, §7, #594): the kind's icon, then two lines. Line one is the title
 * with the actor's name in bold, line two the muted meta — time, then the channel and the live marker — with the
 * action right-aligned. A row whose share, party or activity still exists, or a plugin event with
 * an action, has an action button whose accessible name combines the action and the event, and a
 * click anywhere on the row does the same. The person's name behaves as in the chat (#605): click
 * for the profile preview and menu, double-click for a private message. The feed is not a live
 * region (FR-11.15): rows simply appear in the list.
 */
import type { TFunction } from "i18next";
import { useT } from "../../i18n/useT";
import { clickPerson, doubleClickPerson, ProfilePreviewTrigger } from "../members";
import { ActivityIcon } from "./ActivityIcon";
import type { ActivityEvent } from "./activityStore";

export interface ActivityRowProps {
  event: ActivityEvent;
  /** The row's local observation time, formatted by the feed. */
  time: string;
  /** Whether the share, party or activity the row points at still exists (FR-11.12). */
  live: boolean;
  /** What a click on the row opens; nothing for a plain row. */
  onActivate: (() => void) | undefined;
}

/**
 * The actor's name, as in the chat (#605): a button with the profile preview while the session is
 * known, else the preview by name only. Events from before this connection only know the name,
 * because their session may belong to someone else by now.
 */
function Actor({ event, text }: { event: ActivityEvent; text: string }) {
  if (event.kind === "pluginEvent") {
    return <strong className="activity-row__actor">{text}</strong>;
  }
  const session = event.earlier === true ? null : (event.session ?? null);
  const name = event.name ?? undefined;
  const named = name === undefined ? {} : { name };
  if (session === null) {
    return (
      <ProfilePreviewTrigger session={null} {...named}>
        <strong className="activity-row__actor">{text}</strong>
      </ProfilePreviewTrigger>
    );
  }
  return (
    <ProfilePreviewTrigger session={session} {...named}>
      <button
        type="button"
        className="activity-row__actor activity-row__actor--button"
        onClick={(clicked) => {
          // The name opens the person, not the row's action.
          clicked.stopPropagation();
          clickPerson(clicked.detail, session, clicked.currentTarget, name);
        }}
        onDoubleClick={(clicked) => {
          clicked.stopPropagation();
          doubleClickPerson(session, name);
        }}
      >
        {text}
      </button>
    </ProfilePreviewTrigger>
  );
}

/** Line one's bold part: the actor, "You" for the own session, or the plugin id. */
function actor(event: ActivityEvent, t: TFunction): string {
  if (event.kind === "pluginEvent") return event.plugin.pluginId;
  return event.self ? t("you") : (event.name ?? t("someone"));
}

/** Line one's second part: the verb phrase, or the plugin's own text in quotes (FR-11.11). */
function verb(event: ActivityEvent, t: TFunction): string {
  switch (event.kind) {
    case "connected":
      return t("verb.connected");
    case "disconnected":
      return t("verb.disconnected");
    case "moved":
      return t("verb.moved");
    case "screenShare":
      return t(event.started ? "verb.screenShareStarted" : "verb.screenShareStopped");
    case "watchParty":
      return t(event.started ? "verb.watchPartyStarted" : "verb.watchPartyStopped");
    case "pluginActivity":
      return t(event.started ? "verb.pluginActivityStarted" : "verb.pluginActivityStopped", {
        plugin: event.pluginName,
      });
    default:
      return t("verb.pluginEvent", { text: event.plugin.text });
  }
}

/** Line two's middle part: the channel, or the source word for a plugin's own event (§6). */
function middle(event: ActivityEvent, t: TFunction): string | null {
  if (event.kind === "pluginEvent") return t("plugin");
  if (!("channelName" in event) || event.channelName === null) return null;
  return t("channel", { channel: event.channelName });
}

/** Line two's action: the row's own verb for a live row, the plugin's label for a pushed event. */
function action(event: ActivityEvent, t: TFunction): string | null {
  if (event.kind === "pluginEvent") return event.plugin.actionLabel;
  if (event.kind === "screenShare") return t("watch");
  if (event.kind === "watchParty" || event.kind === "pluginActivity") return t("join");
  return null;
}

/** §7: the clickable row's name says what the action opens; "your" keeps it readable for self. */
function label(event: ActivityEvent, t: TFunction, actionText: string): string {
  if (event.kind === "pluginEvent") {
    return t("rowLabel.pluginEvent", { action: actionText, plugin: event.plugin.pluginId });
  }
  return t(`rowLabel.${event.kind}`, {
    action: actionText,
    name: event.self ? t("selfName") : (event.name ?? t("someone")),
    ...(event.kind === "pluginActivity" ? { plugin: event.pluginName } : {}),
  });
}

export function ActivityRow({ event, time, live, onActivate }: ActivityRowProps) {
  const t = useT("activity");
  const actionText = onActivate === undefined ? null : action(event, t);
  // A share, party or activity that is over: its "stopped" row, or a "started" one no longer live.
  const ended = "live" in event && !live;
  const channelText = middle(event, t);
  const icon = <ActivityIcon event={event} ended={ended} />;
  const title = (
    <span className="activity-row__title">
      <Actor event={event} text={actor(event, t)} />{" "}
      <span className={event.kind === "pluginEvent" ? "activity-row__text" : undefined}>
        {verb(event, t)}
      </span>
    </span>
  );
  const meta = (
    <span className="activity-row__meta">
      <span>{time}</span>
      {channelText !== null && <span>{`· ${channelText}`}</span>}
      {live && <span className="activity-row__live">{`· ${t("live")}`}</span>}
      {actionText !== null && (
        <button
          type="button"
          className="activity-row__action"
          aria-label={label(event, t, actionText)}
          onClick={(clicked) => {
            clicked.stopPropagation();
            onActivate?.();
          }}
        >
          {actionText}
          <span aria-hidden="true">›</span>
        </button>
      )}
    </span>
  );

  const body = (
    <span className="activity-row__body">
      {title}
      {meta}
    </span>
  );
  const className = ["activity-row"];
  if (onActivate !== undefined) className.push("activity-row--clickable");
  if (ended) className.push("activity-row--ended");

  return (
    // A click anywhere on an actionable row does what its action button does; keyboard users reach
    // that button, so the row itself is not focusable.
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-noninteractive-element-interactions
    <li className={className.join(" ")} onClick={actionText === null ? undefined : onActivate}>
      {icon}
      {body}
    </li>
  );
}
