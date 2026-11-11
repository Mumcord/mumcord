import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { MockApi } from "../../../api";
import { getServerState } from "../../channels/serverStore";
import { connectedMock, resetAll } from "../../channels/testUtils";
import { adminCommand, allowedCommands, BUILT_IN_NAMES } from "./commands";
import { peopleOf } from "./run";

let api: MockApi;

/** The server's own answer for one channel, as an admin on the server would change it. */
function grant(channelId: number, permissions: Record<string, boolean>): void {
  const channel = getServerState().channels.get(channelId);
  if (!channel) throw new Error(`the mock has no channel ${String(channelId)}`);
  api.mock.updateChannel(channelId, {
    ...channel,
    permissions: { ...channel.permissions, ...permissions },
  });
}

const names = (): string[] => allowedCommands(getServerState()).map((command) => command.name);

beforeEach(async () => {
  api = await connectedMock({ bindStore: true });
});

afterEach(() => {
  resetAll();
});

describe("which commands the list shows (FR-10.32)", () => {
  it("shows no admin command to a user without a right (FR-10.3)", () => {
    expect(names()).toEqual([]);
  });

  it("shows a channel right as soon as one channel grants it", () => {
    // A moderator in Raid only: the toggles and the moves show, the root rights do not (AC-10.2).
    grant(2, { muteDeafen: true, moveUsers: true });
    expect(names()).toEqual(["mute", "unmute", "deaf", "undeaf", "priority", "move", "moveall"]);
  });

  it("shows the root rights only for the root channel", () => {
    grant(2, { ban: true, kick: true, register: true });
    expect(names()).toEqual([]);
    grant(0, { ban: true, kick: true, register: true });
    expect(names()).toEqual(["register", "unban", "kick", "ban"]);
  });
});

describe("the people a command may name (FR-10.32)", () => {
  it("offers only the people whose channel grants the right", () => {
    // The mock has Anna and Ben in Lobby and Carla in Raid.
    grant(2, { muteDeafen: true });
    const mute = adminCommand("mute");
    if (!mute) throw new Error("the command set lacks /mute");
    expect(peopleOf(mute, getServerState()).map((person) => person.name)).toEqual(["Carla"]);

    grant(1, { muteDeafen: true });
    expect(peopleOf(mute, getServerState()).map((person) => person.name)).toEqual([
      "Anna",
      "Ben",
      "Carla",
    ]);
  });

  it("never offers yourself", () => {
    grant(1, { muteDeafen: true });
    const mute = adminCommand("mute");
    if (!mute) throw new Error("the command set lacks /mute");
    const self = getServerState().selfSession;
    expect(peopleOf(mute, getServerState()).some((person) => person.item.session === self)).toBe(
      false,
    );
  });

  it("offers only guests to /register (AC-10.15)", () => {
    grant(0, { register: true });
    grant(1, { muteDeafen: true });
    const register = adminCommand("register");
    if (!register) throw new Error("the command set lacks /register");
    // Ben has no certificate, so he is the only guest of the mock.
    expect(peopleOf(register, getServerState()).map((person) => person.name)).toEqual(["Ben"]);
  });
});

describe("the built-in names (FR-10.30)", () => {
  it("holds every command of the list, so a plugin cannot take one", () => {
    expect([...BUILT_IN_NAMES].sort()).toEqual([
      "ban",
      "deaf",
      "kick",
      "move",
      "moveall",
      "mute",
      "priority",
      "register",
      "unban",
      "undeaf",
      "unmute",
    ]);
  });
});
