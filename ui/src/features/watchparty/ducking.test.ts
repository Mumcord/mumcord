import { afterEach, describe, expect, it, vi } from "vitest";
import type { User } from "../../api";
import type { ServerState } from "../channels/serverStore";
import { Ducker, RELEASE_DELAY_MS, someoneTalking } from "./ducking";
import { volumeCommand } from "./playerBridge";

const SELF = 1;
const HERE = 0;
const ELSEWHERE = 5;

function user(session: number, channelId: number, localMute = false): User {
  return { session, channelId, localMute } as User;
}

function state(talking: number[], users: User[]): ServerState {
  return {
    selfSession: SELF,
    users: new Map([user(SELF, HERE), ...users].map((u) => [u.session, u])),
    talking: new Set(talking),
  } as unknown as ServerState;
}

describe("someone talking (FR-2.76)", () => {
  it("counts the user and people in the own channel", () => {
    expect(someoneTalking(state([], [user(2, HERE)]))).toBe(false);
    expect(someoneTalking(state([SELF], []))).toBe(true);
    expect(someoneTalking(state([2], [user(2, HERE)]))).toBe(true);
  });

  it("ignores other channels and locally muted people", () => {
    expect(someoneTalking(state([2], [user(2, ELSEWHERE)]))).toBe(false);
    expect(someoneTalking(state([3], [user(3, HERE, true)]))).toBe(false);
  });

  it("is false while not connected", () => {
    const disconnected = { ...state([SELF], []), selfSession: null };
    expect(someoneTalking(disconnected)).toBe(false);
  });
});

describe("setVolume bridge command", () => {
  it("carries a level clamped to 0–1 and nothing else", () => {
    expect(volumeCommand(0.3)).toEqual({ type: "setVolume", level: 0.3 });
    expect(volumeCommand(2)).toEqual({ type: "setVolume", level: 1 });
    expect(volumeCommand(-1)).toEqual({ type: "setVolume", level: 0 });
    expect(volumeCommand(Number.NaN)).toBeNull();
  });
});

describe("ducker", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  const ducker = () => {
    vi.useFakeTimers();
    const d = new Ducker();
    const levels: number[] = [];
    d.subscribe((level) => levels.push(level));
    return { d, levels };
  };

  it("lowers to 30 % at once and comes back after the release delay", () => {
    const { d, levels } = ducker();
    expect(d.level).toBe(1);
    d.setTalking(true);
    expect(levels).toEqual([0.3]);
    d.setTalking(false);
    vi.advanceTimersByTime(RELEASE_DELAY_MS - 1);
    expect(d.level).toBe(0.3);
    vi.advanceTimersByTime(1);
    expect(levels).toEqual([0.3, 1]);
  });

  it("stays lowered when speech resumes within the delay", () => {
    const { d, levels } = ducker();
    d.setTalking(true);
    d.setTalking(false);
    vi.advanceTimersByTime(RELEASE_DELAY_MS / 2);
    d.setTalking(true);
    vi.advanceTimersByTime(RELEASE_DELAY_MS * 2);
    expect(levels).toEqual([0.3]);
  });

  it("follows the slider and does nothing while switched off", () => {
    const { d, levels } = ducker();
    d.setSettings({ videoDucking: true, videoDuckingPercent: 60 });
    d.setTalking(true);
    expect(d.level).toBe(0.6);
    d.setSettings({ videoDucking: false, videoDuckingPercent: 60 });
    expect(d.level).toBe(1);
    d.setTalking(false);
    d.setTalking(true);
    expect(levels).toEqual([0.6, 1]);
  });
});
