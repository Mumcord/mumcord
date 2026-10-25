import type {
  BlocklistInfo,
  GameKey,
  GameOverlayEntry,
  InGameOverlayState,
  MumcordApi,
  OverlayCorner,
} from "../../../api";

/** Global switch of the in-game overlay (FR-1.46b): "off" never loads a layer or hook. */
export type InGameMode = "off" | "on";

/** Why a game is on the anti-cheat blocklist (QA.56). */
export interface InGameBlock {
  /** Short, sourced reason, e.g. the anti-cheat product. Untrusted text. */
  reason: string;
}

/** Per-game look of the in-game overlay. */
export interface InGameGameOptions {
  corner: OverlayCorner;
  /** Percent of the default size, 50 – 200. */
  size: number;
  /** Percent, 20 – 100. */
  opacity: number;
}

export interface InGameGame {
  id: string;
  name: string;
  /** `data:` URL of the game's icon, if known. */
  icon: string | null;
  /** The game is running right now. */
  detected: boolean;
  /** `null` follows the global mode (and the blocklist); true/false is the user's choice. */
  override: boolean | null;
  /** Set for blocklisted games: off by default even when the global mode is on. */
  blocked: InGameBlock | null;
  options: InGameGameOptions;
}

export interface InGameState {
  /** False where no in-game overlay exists (macOS): the window overlay is the fallback. */
  supported: boolean;
  /** i18n key (namespace `settings`) naming why it is not supported, or `null`. */
  explanationKey: string | null;
  mode: InGameMode;
  games: InGameGame[];
}

/** What the settings need from the in-game overlay backend; see [`createInGameOverlayApi`]. */
export interface InGameOverlayApi {
  state: () => Promise<InGameState>;
  setMode: (mode: InGameMode) => Promise<InGameState>;
  /** `null` goes back to the default (global mode and blocklist). */
  setGameOverride: (id: string, enabled: boolean | null) => Promise<InGameState>;
  setGameOptions: (id: string, options: InGameGameOptions) => Promise<InGameState>;
  /** Calls `onChange` whenever the settings change (also from elsewhere); returns the unsubscribe. */
  subscribe: (onChange: (state: InGameState) => void) => () => void;
}

/** Whether the overlay is effectively on for a game under the given global mode. */
export function effectivelyOn(game: InGameGame, mode: InGameMode): boolean {
  return game.override ?? (mode === "on" && game.blocked === null);
}

/** Defaults of a game the user has not configured yet (match the engine's store defaults). */
const DEFAULT_OPTIONS: InGameGameOptions = { corner: "topLeft", size: 100, opacity: 90 };

const idOf = (key: GameKey) => `${key.kind}:${String(key.value)}`;

const blockOf = (blocklist: BlocklistInfo | null): InGameBlock | null =>
  blocklist === null ? null : { reason: `${blocklist.antiCheat}: ${blocklist.reason}` };

/**
 * Engine entries for every game the settings list: configured ones as stored, known ones (running
 * this session or on the blocklist) as fresh entries with the default look.
 */
function entriesOf(state: InGameOverlayState): GameOverlayEntry[] {
  const known = state.known.map<GameOverlayEntry>((game) => ({
    key: game.key,
    name: game.name,
    enabled: null,
    riskConfirmed: false,
    corner: DEFAULT_OPTIONS.corner,
    sizePercent: DEFAULT_OPTIONS.size,
    opacityPercent: DEFAULT_OPTIONS.opacity,
    showWholeChannel: false,
    showChannelName: true,
    blocklist: game.blocklist,
    effective: "globalOff",
  }));
  return [...state.games, ...known];
}

function toInGameState(state: InGameOverlayState): InGameState {
  const running = new Set(state.known.filter((g) => g.running).map((g) => idOf(g.key)));
  return {
    supported: state.supported,
    explanationKey: state.explanationKey,
    mode: state.mode,
    games: entriesOf(state).map((entry) => ({
      id: idOf(entry.key),
      name: entry.name,
      icon: null,
      detected: running.has(idOf(entry.key)),
      override: entry.enabled,
      blocked: blockOf(entry.blocklist),
      options: { corner: entry.corner, size: entry.sizePercent, opacity: entry.opacityPercent },
    })),
  };
}

/**
 * The settings' view of the engine's in-game overlay commands. Games are addressed by an id
 * derived from their key; changes are sent as whole entries built from the last known state.
 */
export function createInGameOverlayApi(api: MumcordApi): InGameOverlayApi {
  let last: InGameOverlayState | null = null;
  const remember = (state: InGameOverlayState) => {
    last = state;
    return toInGameState(state);
  };
  const entry = (id: string): GameOverlayEntry => {
    const found = last === null ? undefined : entriesOf(last).find((e) => idOf(e.key) === id);
    if (found === undefined) throw new Error(`Unknown game for the in-game overlay: ${id}`);
    return found;
  };
  const save = async (next: GameOverlayEntry) => remember(await api.ingameOverlaySetGame(next));
  return {
    state: async () => remember(await api.ingameOverlayState()),
    setMode: async (mode) => remember(await api.ingameOverlaySetMode(mode)),
    setGameOverride: async (id, enabled) => {
      const current = entry(id);
      // The settings only enable a blocklisted game after the user confirmed the stronger warning.
      const riskConfirmed = enabled === true && current.blocklist !== null;
      return save({ ...current, enabled, riskConfirmed });
    },
    setGameOptions: async (id, options) =>
      save({
        ...entry(id),
        corner: options.corner,
        sizePercent: options.size,
        opacityPercent: options.opacity,
      }),
    subscribe: (onChange) =>
      api.on("inGameOverlayChanged", ({ state }) => {
        onChange(remember(state));
      }),
  };
}
