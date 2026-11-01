/**
 * Keeps the local player in step with the party (FR-2.40). Party changes are applied to the
 * player; what the user does in the player (play, pause, jumping to another time) becomes a
 * command for everyone. The player's own reactions to an applied change are ignored for a
 * moment, so a remote pause never echoes back as a local one. When the video ends, the party goes
 * on to the next playlist entry (FR-2.69).
 */
import type { WatchCommand, WatchParty } from "../../api";
import { positionAt, type LiveParty } from "./watchPartyStore";
import { ENDED, PAUSED, PLAYING } from "./youtube";

/** After applying a change, player reports are its reaction, not the user. */
const QUIET_MS = 1500;
/** A jump between two time reports larger than this is the user seeking. */
const JUMP_MS = 2000;
/** Drifting further than this from the party is corrected locally (e.g. after buffering). */
const DRIFT_MS = 2500;

export interface PlayerControl {
  play(): void;
  pause(): void;
  seekTo(positionMs: number): void;
}

export interface SyncDeps {
  now: () => number;
  player: PlayerControl;
  send: (command: WatchCommand) => void;
  /** The video of playlist entry `entry` ended here. */
  ended: (entry: number) => void;
}

export class PartySync {
  private live: LiveParty | null = null;
  private ready = false;
  private quietUntil = 0;
  private playerState: number | null = null;
  private lastTime: { ms: number; at: number } | null = null;
  /** The entry whose video ended here, and the party's last action at that moment. */
  private endedAt: { entry: number; action: WatchParty["lastAction"] } | null = null;

  constructor(private readonly deps: SyncDeps) {}

  /** The party changed (or was first shown). */
  update(live: LiveParty): void {
    this.live = live;
    if (this.ready) this.apply();
  }

  /** The user acted here: send it for everyone and ignore the player's own echo (issue #163). */
  act(command: WatchCommand): void {
    this.deps.send(command);
    this.quietUntil = this.deps.now() + QUIET_MS;
  }

  /** The player answers the control protocol. */
  onReady(): void {
    if (this.ready) return;
    this.ready = true;
    this.apply();
  }

  onState(state: number): void {
    // Where the player was when its state changed, by the state it had until now.
    const positionMs = this.playerMs();
    this.playerState = state;
    const live = this.live;
    if (live === null) return;
    // Every client reports the end; the engine goes on only once per entry.
    if (state === ENDED) {
      this.endedAt = { entry: live.party.currentEntry, action: live.party.lastAction };
      this.deps.ended(live.party.currentEntry);
      return;
    }
    if (this.quiet() || this.lastTime === null) return;
    if (state === PLAYING && !live.party.playing) this.deps.send({ kind: "play", positionMs });
    else if (state === PAUSED && live.party.playing) this.deps.send({ kind: "pause", positionMs });
  }

  onTime(seconds: number): void {
    const now = this.deps.now();
    const ms = Math.max(0, Math.round(seconds * 1000));
    const previous = this.lastTime;
    this.lastTime = { ms, at: now };
    const live = this.live;
    if (live === null || previous === null || this.quiet()) return;
    const advance = this.playerState === PLAYING ? now - previous.at : 0;
    if (Math.abs(ms - (previous.ms + advance)) > JUMP_MS) {
      this.deps.send({ kind: "seek", positionMs: ms });
      return;
    }
    if (live.party.playing && Math.abs(ms - positionAt(live, now)) > DRIFT_MS) this.apply();
  }

  private quiet(): boolean {
    return this.deps.now() < this.quietUntil;
  }

  /** The player's position now, from its last time report. */
  private playerMs(): number {
    const last = this.lastTime;
    if (last === null) return 0;
    const advance = this.playerState === PLAYING ? this.deps.now() - last.at : 0;
    return Math.max(0, Math.round(last.ms + advance));
  }

  /**
   * Whether the video ended here and nobody acted on it since. Seeking or playing it would
   * restart it, and the party's position runs past its end until the engine has caught up.
   */
  private restsAtEnd(live: LiveParty): boolean {
    const ended = this.endedAt;
    if (ended === null || ended.entry !== live.party.currentEntry) return false;
    const [a, b] = [ended.action, live.party.lastAction];
    return (
      a?.session === b?.session &&
      a?.command.kind === b?.command.kind &&
      a?.command.positionMs === b?.command.positionMs
    );
  }

  private apply(): void {
    const live = this.live;
    if (live === null || this.restsAtEnd(live)) return;
    const now = this.deps.now();
    const target = Math.round(positionAt(live, now));
    let changed = false;
    if (this.lastTime === null || Math.abs(this.playerMs() - target) > JUMP_MS / 2) {
      this.deps.player.seekTo(target);
      this.lastTime = { ms: target, at: now };
      changed = true;
    }
    if (live.party.playing && this.playerState !== PLAYING) {
      this.deps.player.play();
      changed = true;
    } else if (!live.party.playing && this.playerState !== PAUSED) {
      this.deps.player.pause();
      changed = true;
    }
    // Only the player's reaction to a change is ignored: after the echo of the user's own pause
    // (e.g. while dragging the seek bar) the seek and the resume that follow must still go out.
    if (changed) this.quietUntil = now + QUIET_MS;
  }
}
