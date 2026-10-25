import type { OverlayCorner } from "../../../api";

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
  mode: InGameMode;
  games: InGameGame[];
}

/**
 * What the settings need from the in-game overlay backend. Not part of the generated
 * bindings yet: wire it to the engine commands once they exist.
 */
export interface InGameOverlayApi {
  state: () => Promise<InGameState>;
  setMode: (mode: InGameMode) => Promise<InGameState>;
  /** `null` goes back to the default (global mode and blocklist). */
  setGameOverride: (id: string, enabled: boolean | null) => Promise<InGameState>;
  setGameOptions: (id: string, options: InGameGameOptions) => Promise<InGameState>;
}

/** Whether the overlay is effectively on for a game under the given global mode. */
export function effectivelyOn(game: InGameGame, mode: InGameMode): boolean {
  return game.override ?? (mode === "on" && game.blocked === null);
}

/** Placeholder until the engine commands exist: nothing supported, no games. */
export const unavailableInGameApi: InGameOverlayApi = (() => {
  const state: InGameState = { supported: false, mode: "off", games: [] };
  const resolve = () => Promise.resolve(state);
  return { state: resolve, setMode: resolve, setGameOverride: resolve, setGameOptions: resolve };
})();
