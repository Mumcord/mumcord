/**
 * Feeds the notification centre's Plugins category (FR-6.4, FR-6.22, FR-6.69, FR-6.79): `notify`
 * entries, "Turned off after repeated crashes", available updates and server suggestions. Never
 * an OS notification; Do Not Disturb is applied by the host for `notify`, and the centre itself
 * always collects silently (Q1.5).
 */
import type { MumcordApi, Unsubscribe } from "../../../api";
import { addItem, getCentreState, removeItems } from "../../notifications/centreStore";
import type { PluginNotice } from "./types";

/** Long enough for any sensible title or line, short enough to keep the list tidy. */
const MAX_TITLE = 80;
const MAX_BODY = 300;

function capped(text: string, max: number): string {
  const chars = Array.from(text.replace(/\s+/g, " ").trim());
  return chars.length <= max
    ? chars.join("")
    : `${chars
        .slice(0, max - 1)
        .join("")
        .trimEnd()}…`;
}

export function pluginNoticeId(notice: PluginNotice): string {
  switch (notice.kind) {
    case "message":
      // Several messages of one plugin are separate entries.
      return `plugin/message/${notice.plugin}/${String(Date.now())}/${Math.random().toString(36).slice(2, 8)}`;
    case "autoOff":
      return `plugin/autoOff/${notice.plugin}`;
    case "update":
      return `plugin/update/${notice.plugin}`;
    case "suggestion":
      return `plugin/suggestion/${String(notice.serverId)}/${notice.plugin}`;
  }
}

export function recordPluginNotice(notice: PluginNotice): void {
  addItem({ kind: "plugin", id: pluginNoticeId(notice), at: Date.now(), read: false, notice });
}

/** Names of the installed plugins, so a `notify` entry carries its plugin's name. */
async function installedNames(api: MumcordApi): Promise<Map<string, string>> {
  const { plugins } = await api.pluginsList();
  return new Map(plugins.map((plugin) => [plugin.id, plugin.name]));
}

export function bindPluginNotices(api: MumcordApi): Unsubscribe {
  const unsubscribers = [
    api.on("pluginsNotification", ({ plugin, notice }) => {
      const record = (name: string | undefined) => {
        const shown = name || plugin;
        switch (notice.kind) {
          case "message":
            recordPluginNotice({
              kind: "message",
              plugin,
              name: shown,
              title: capped(notice.title, MAX_TITLE),
              body: capped(notice.body, MAX_BODY),
            });
            return;
          case "autoOff":
            recordPluginNotice({ kind: "autoOff", plugin, name: shown });
            return;
          case "updateAvailable":
            recordPluginNotice({
              kind: "update",
              plugin,
              name: capped(notice.name, MAX_TITLE),
              version: capped(notice.version, MAX_TITLE),
            });
            return;
        }
      };
      if (notice.kind === "updateAvailable") {
        record(undefined);
        return;
      }
      installedNames(api).then(
        (names) => {
          record(names.get(plugin));
        },
        (error: unknown) => {
          console.error("Reading the plugin names failed", error);
          record(undefined);
        },
      );
    }),
    api.on("pluginsSuggestion", (suggestion) => {
      const notice: PluginNotice = {
        kind: "suggestion",
        plugin: suggestion.plugin,
        name: capped(suggestion.name, MAX_TITLE),
        serverId: suggestion.serverId,
        reason: capped(suggestion.reason, MAX_BODY),
        seq: suggestion.seq,
        source:
          suggestion.source.kind === "official"
            ? { kind: "official", version: suggestion.source.version }
            : { kind: "repository", url: suggestion.source.url, key: suggestion.source.key },
      };
      // The server repeats its list now and then; the same list does not become unread again.
      const known = getCentreState().items.find((item) => item.id === pluginNoticeId(notice));
      if (
        known?.kind === "plugin" &&
        known.notice.kind === "suggestion" &&
        known.notice.seq === notice.seq
      ) {
        return;
      }
      recordPluginNotice(notice);
    }),
    // Installing a suggested plugin, by any route, answers the suggestion.
    api.on("pluginsChanged", () => {
      installedNames(api).then(
        (names) => {
          removeItems(
            (item) =>
              item.kind === "plugin" &&
              item.notice.kind === "suggestion" &&
              names.has(item.notice.plugin),
          );
        },
        (error: unknown) => {
          console.error("Reading the installed plugins failed", error);
        },
      );
    }),
  ];
  return () => {
    for (const unsubscribe of unsubscribers) unsubscribe();
  };
}
