/**
 * The icon in front of each feed row (#594, mockup style B): the app's 24px line icon in a circle
 * tinted per kind, grey once the row's share, party or activity has ended. A plugin's own event shows
 * the plugin's icon unframed, or its letter tile when it has none. Decorative: the row text already
 * says what happened, so screen readers skip it.
 */
import type { ReactNode } from "react";
import { usePlugins } from "../plugins/pluginStore";
import { usePluginIcon } from "../plugins/render/usePluginIcon";
import { PluginTile } from "../plugins/render/PluginTile";
import type { ActivityEvent } from "./activityStore";

const SHAPES = {
  join: (
    <>
      <path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4" />
      <path d="M10 17l5-5-5-5" />
      <path d="M15 12H3" />
    </>
  ),
  left: (
    <>
      <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />
      <path d="M16 17l5-5-5-5" />
      <path d="M21 12H9" />
    </>
  ),
  move: (
    <>
      <path d="M5 12h14" />
      <path d="M13 6l6 6-6 6" />
    </>
  ),
  screen: (
    <>
      <rect x="2" y="3" width="20" height="14" rx="2" />
      <path d="M8 21h8M12 17v4" />
    </>
  ),
  watch: <polygon points="6 3 20 12 6 21 6 3" />,
  plug: (
    <>
      <path d="M12 22v-5" />
      <path d="M9 8V2" />
      <path d="M15 8V2" />
      <path d="M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z" />
    </>
  ),
} as const;

type Shape = keyof typeof SHAPES;

/** The line icon of every kind but the plugin's own event, which shows the plugin's icon. */
const SHAPE_OF: Record<Exclude<ActivityEvent["kind"], "pluginEvent">, Shape> = {
  connected: "join",
  disconnected: "left",
  moved: "move",
  screenShare: "screen",
  watchParty: "watch",
  pluginActivity: "plug",
};

function PluginEventIcon({ pluginId }: { pluginId: string }) {
  const url = usePluginIcon(pluginId);
  const { overview } = usePlugins();
  const name = overview?.plugins.find((plugin) => plugin.id === pluginId)?.name ?? pluginId;
  return (
    <span className="activity-icon activity-icon--plugin" aria-hidden="true">
      {url === null ? (
        <PluginTile id={pluginId} name={name} className="activity-icon__tile" />
      ) : (
        <img className="activity-icon__image" src={url} alt="" />
      )}
    </span>
  );
}

export function ActivityIcon({
  event,
  ended,
}: {
  event: ActivityEvent;
  /** The row's share, party or activity is over, so the icon turns grey. */
  ended: boolean;
}): ReactNode {
  if (event.kind === "pluginEvent") return <PluginEventIcon pluginId={event.plugin.pluginId} />;
  const shape = SHAPE_OF[event.kind];
  return (
    <span
      className={`activity-icon activity-icon--${event.kind}`}
      data-ended={ended || undefined}
      data-shape={shape}
      aria-hidden="true"
    >
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        focusable="false"
      >
        {SHAPES[shape]}
      </svg>
    </span>
  );
}
