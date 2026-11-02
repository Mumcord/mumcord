/** What a plugin entry of the notification centre says (FR-6.4, FR-6.22, FR-6.69, FR-6.79). */

/** Where a suggested plugin comes from; mirrors the engine's event. */
export type SuggestionSource =
  { kind: "official"; version: string } | { kind: "repository"; url: string; key: string | null };

/**
 * All texts are untrusted plain text: they come from a plugin or from a server and are only ever
 * rendered as text, never as markup.
 */
export type PluginNotice =
  /** `notify.send` from a running plugin. */
  | { kind: "message"; plugin: string; name: string; title: string; body: string }
  /** "Turned off after repeated crashes". */
  | { kind: "autoOff"; plugin: string; name: string }
  /** A newer version waits in the Updates tab. */
  | { kind: "update"; plugin: string; name: string; version: string }
  /** A registered bot of the server suggests a plugin. */
  | {
      kind: "suggestion";
      plugin: string;
      name: string;
      serverId: number;
      reason: string;
      seq: string;
      source: SuggestionSource;
    };
