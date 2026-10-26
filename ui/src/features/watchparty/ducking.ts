/**
 * Auto-ducking (FR-2.76): while anyone in the own voice channel talks (the user included, people
 * muted locally not), the watch-party video and watched screen-share sound drop to the lowered
 * level from the settings. The state is decided once here from the server store's talking state,
 * never per frame in React; the player page and the engine's share mixer do the fades.
 */
import type { Settings } from "../../api";
import { selfUser, type ServerState } from "../channels/serverStore";

/**
 * Wait after the last speech before the sound comes back. The engine already holds a talk stop
 * for 150 ms, so the level rises about 800 ms after someone stopped talking.
 */
export const RELEASE_DELAY_MS = 650;

export type DuckingSettings = Pick<Settings, "videoDucking" | "videoDuckingPercent">;

/** Whether someone in the own voice channel talks right now, ignoring locally muted people. */
export function someoneTalking(state: ServerState): boolean {
  const self = selfUser(state);
  if (self === undefined) return false;
  for (const session of state.talking) {
    if (session === self.session) return true;
    const user = state.users.get(session);
    if (user !== undefined && user.channelId === self.channelId && !user.localMute) return true;
  }
  return false;
}

interface Timers {
  set: (run: () => void, ms: number) => number;
  clear: (id: number) => void;
}

const windowTimers: Timers = {
  set: (run, ms) => window.setTimeout(run, ms),
  clear: (id) => {
    window.clearTimeout(id);
  },
};

/** Turns talking on/off into the video level (1 = normal), with the release delay. */
export class Ducker {
  private enabled = true;
  private percent = 30;
  private ducked = false;
  private release: number | null = null;
  private readonly listeners = new Set<(level: number) => void>();

  constructor(private readonly timers: Timers = windowTimers) {}

  /** The video level now: 1, or the lowered fraction while ducked. */
  get level(): number {
    return this.enabled && this.ducked ? this.percent / 100 : 1;
  }

  setSettings(settings: DuckingSettings): void {
    this.change(() => {
      this.enabled = settings.videoDucking;
      this.percent = settings.videoDuckingPercent;
    });
  }

  setTalking(talking: boolean): void {
    if (talking) {
      this.cancelRelease();
      this.change(() => {
        this.ducked = true;
      });
    } else if (this.ducked && this.release === null) {
      this.release = this.timers.set(() => {
        this.release = null;
        this.change(() => {
          this.ducked = false;
        });
      }, RELEASE_DELAY_MS);
    }
  }

  /** Test helper: back to the defaults, not ducked, without telling the listeners. */
  reset(): void {
    this.cancelRelease();
    this.enabled = true;
    this.percent = 30;
    this.ducked = false;
  }

  /** Calls `listener` with every new level; returns an unsubscribe function. */
  subscribe(listener: (level: number) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private cancelRelease(): void {
    if (this.release === null) return;
    this.timers.clear(this.release);
    this.release = null;
  }

  private change(update: () => void): void {
    const before = this.level;
    update();
    const after = this.level;
    if (after !== before) for (const listener of this.listeners) listener(after);
  }
}

/** The app's one ducker; fed by `VideoDucking` and the settings. */
export const ducker = new Ducker();
