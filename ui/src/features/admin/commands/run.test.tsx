import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "../../../i18n";
import type { Conversation, MockApi } from "../../../api";
import { getServerState } from "../../channels/serverStore";
import { connectedMock, resetAll, stubDialog } from "../../channels/testUtils";
import { privateLinesFor, resetPrivateLines } from "../../chat/privateLines";
import { AdminConfirmHost } from "./AdminConfirmHost";
import { dismissConfirm, resetConfirm } from "./confirmStore";
import { PersonActionHost } from "../PersonActionHost";
import { resetPersonAction } from "../personActionStore";
import { runBuiltIn, type RunResult } from "./run";
import type { CommandContext } from "./context";

/**
 * The commands typed in the chat box, run against the mock server: what they ask the engine for,
 * the line they leave behind and the dialog they prefill (Epic 4, issue #378, FR-10.30 ff.).
 */
const CHANNEL: Conversation = { kind: "channel", channelId: 1 };
const ANNA = 2;
const BEN = 3;

let api: MockApi;

const adminT = i18n.getFixedT("en", "admin");

function ctx(): CommandContext {
  return { serverId: 1, conversation: CHANNEL, state: getServerState(), t: adminT };
}

/** Grants a right the way the server's own answer for a channel does (FR-10.1). */
function grant(channelId: number, permissions: Record<string, boolean>): void {
  const channel = getServerState().channels.get(channelId);
  if (!channel) throw new Error(`the mock has no channel ${String(channelId)}`);
  api.mock.updateChannel(channelId, {
    ...channel,
    permissions: { ...channel.permissions, ...permissions },
  });
}

/** The lines only this admin sees in the chat the command was typed in (FR-10.35). */
const lines = (): string[] =>
  privateLinesFor(1, CHANNEL).map((line) => `${line.message.body} · ${line.note}`);

/** Runs a command and returns what it wants to show above the box. */
async function typed(line: string): Promise<RunResult> {
  return await runBuiltIn(line, ctx());
}

/** The question `/unban` or `/moveall` asks, with the server it belongs to (FR-10.58). */
async function asked(title: string): Promise<HTMLElement> {
  render(<AdminConfirmHost />);
  return await screen.findByRole("dialog", { name: title });
}

beforeEach(async () => {
  api = await connectedMock({ bindStore: true });
  stubDialog();
  resetPrivateLines();
  resetConfirm();
  resetPersonAction();
});

afterEach(() => {
  dismissConfirm();
  resetConfirm();
  resetPersonAction();
  resetAll();
});

describe("a command that acts at once (FR-10.15, FR-10.35)", () => {
  it("mutes on the server and leaves a private line in that chat", async () => {
    grant(1, { muteDeafen: true });
    const mute = vi.spyOn(api, "personSetServerMute");
    expect((await typed("/mute Anna")).kind).toBe("done");
    await waitFor(() => {
      expect(mute).toHaveBeenCalledWith(1, ANNA, true);
    });
    expect(lines()).toEqual(["You muted Anna on the server. · Only you see this line"]);
    // The line names no sender: it is what the server did, not a message (mockup 05).
    expect(privateLinesFor(1, CHANNEL)[0]?.message.senderName).toBe("");
  });

  it("toggles priority speaker, which is what the line reports (FR-10.30)", async () => {
    grant(1, { muteDeafen: true });
    const priority = vi.spyOn(api, "personSetPrioritySpeaker");
    await typed("/priority Anna");
    await waitFor(() => {
      expect(priority).toHaveBeenCalledWith(1, ANNA, true);
    });
    await typed("/priority Anna");
    await waitFor(() => {
      expect(priority).toHaveBeenLastCalledWith(1, ANNA, false);
    });
    expect(lines()).toEqual([
      "You made Anna a priority speaker. · Only you see this line",
      "Anna is no longer a priority speaker. · Only you see this line",
    ]);
  });

  it("registers a guest, and no one who is registered already (FR-10.20, AC-10.15)", async () => {
    grant(0, { register: true });
    expect(await typed("/register Anna")).toEqual({
      kind: "notice",
      text: 'No one named "Anna" is on this server right now.',
    });
    // An account is bound to the client certificate, so only a guest with one can be registered.
    const ben = getServerState().users.get(BEN);
    if (!ben) throw new Error("seed person missing");
    act(() => {
      api.mock.upsertUser({ ...ben, certHash: "ab".repeat(20) });
    });
    const register = vi.spyOn(api, "personRegister");
    await typed("/register Ben");
    await waitFor(() => {
      expect(register).toHaveBeenCalledWith(1, BEN);
    });
    expect(lines()).toEqual(["Ben is registered on the server. · Only you see this line"]);
  });

  it("moves one person to a named channel (FR-10.30)", async () => {
    grant(1, { moveUsers: true });
    grant(2, { moveUsers: true });
    const move = vi.spyOn(api, "personMove");
    expect((await typed("/move Anna Raid")).kind).toBe("done");
    await waitFor(() => {
      expect(move).toHaveBeenCalledWith(1, ANNA, 2);
    });
    expect(lines()).toEqual(["You moved Anna to Raid. · Only you see this line"]);
    expect(getServerState().users.get(ANNA)?.channelId).toBe(2);
  });

  it("refuses a command whose right the server then takes away (FR-10.59)", async () => {
    grant(1, { muteDeafen: true });
    api.mock.failNext("personSetServerMute", "permissionDenied", "mute right in the channel");
    expect(await typed("/mute Anna")).toEqual({
      kind: "notice",
      text: "The server refused: you do not have that right on the root channel.",
    });
    expect(lines()).toEqual([]);
  });
});

describe("a command the admin may not use (FR-10.33, Q1.10)", () => {
  it("does nothing at all: nothing is sent and no line appears", async () => {
    const mute = vi.spyOn(api, "personSetServerMute");
    const send = vi.spyOn(api, "sendChannelMessage");
    expect((await typed("/mute Anna")).kind).toBe("done");
    expect(mute).not.toHaveBeenCalled();
    expect(send).not.toHaveBeenCalled();
    expect(lines()).toEqual([]);
  });
});

describe("what an argument names (FR-10.34)", () => {
  it("asks for a name when the command got none, with its usage", async () => {
    grant(1, { muteDeafen: true });
    expect(await typed("/mute")).toEqual({ kind: "notice", text: "/mute takes name" });
  });

  it("says when no one matches and suggests the closest name (mockup 04b)", async () => {
    grant(1, { muteDeafen: true });
    expect(await typed("/mute Annaa")).toEqual({
      kind: "notice",
      text: 'No one named "Annaa" is on this server right now. Did you mean Anna?',
    });
  });

  it("asks which person was meant when several match (mockup 04c)", async () => {
    grant(1, { muteDeafen: true });
    const anna = getServerState().users.get(ANNA);
    if (!anna) throw new Error("the mock has no Anna");
    api.mock.upsertUser({ ...anna, session: 20, name: "Annabel" });
    const result = await typed("/mute Ann");
    expect(result.kind).toBe("choice");
    if (result.kind !== "choice") return;
    expect(result.options.map((option) => option.keyword)).toEqual(["Anna", "Annabel"]);
    expect(result.options[0]?.group).toBe("Which one did you mean?");

    // Choosing one of them runs the command for that person only.
    const mute = vi.spyOn(api, "personSetServerMute");
    result.options[1]?.run?.();
    await waitFor(() => {
      expect(mute).toHaveBeenCalledWith(1, 20, true);
    });
    expect(lines()).toEqual(["You muted Annabel on the server. · Only you see this line"]);
  });

  it("reads a name with a space as one person, and the words after it as the reason", async () => {
    grant(1, { muteDeafen: true });
    const anna = getServerState().users.get(ANNA);
    if (!anna) throw new Error("the mock has no Anna");
    act(() => {
      api.mock.upsertUser({ ...anna, session: 21, name: "Bob" });
      api.mock.upsertUser({ ...anna, session: 22, name: "Bob Smith" });
    });
    const mute = vi.spyOn(api, "personSetServerMute");
    await typed("/mute Bob Smith");
    await waitFor(() => {
      expect(mute).toHaveBeenLastCalledWith(1, 22, true);
    });
    await typed("/mute Bob spamming");
    await waitFor(() => {
      expect(mute).toHaveBeenLastCalledWith(1, 21, true);
    });
    expect(mute).toHaveBeenCalledTimes(2);
    // The words after the full name stay the reason.
    grant(0, { kick: true });
    await typed("/kick Bob Smith spamming");
    render(<PersonActionHost />);
    const dialog = await screen.findByRole("dialog", {
      name: "Kick Bob Smith? · Local test server",
    });
    expect(within(dialog).getByLabelText("Reason")).toHaveValue("spamming");
  });

  it("never runs on a partial name; the admin picks from the list (FR-10.34)", async () => {
    grant(1, { muteDeafen: true });
    const mute = vi.spyOn(api, "personSetServerMute");
    const result = await typed("/mute An");
    expect(result.kind).toBe("choice");
    if (result.kind === "choice") {
      expect(result.options.map((option) => option.keyword)).toEqual(["Anna"]);
    }
    expect(mute).not.toHaveBeenCalled();
  });

  it("names a channel, and says so when none fits (FR-10.34)", async () => {
    grant(1, { moveUsers: true });
    grant(2, { moveUsers: true });
    expect(await typed("/move Anna Raid 2")).toEqual({
      kind: "notice",
      text: 'There is no channel named "Raid 2" on this server.',
    });
    expect(await typed("/move Anna")).toEqual({
      kind: "notice",
      text: "Which channel? Type its name after the command.",
    });
  });
});

describe("the commands that ask first (FR-10.55)", () => {
  it("/ban opens the ban dialog prefilled with the duration and the reason (FR-10.31)", async () => {
    grant(0, { ban: true });
    await typed("/ban Anna 7d spam links");
    render(<PersonActionHost />);
    const dialog = await screen.findByRole("dialog", { name: "Ban Anna · Local test server" });
    expect(within(dialog).getByLabelText("Reason")).toHaveValue("spam links");
    const pressed = within(dialog)
      .getAllByRole("button", { pressed: true })
      .map((button) => button.textContent);
    expect(pressed).toContain("7 days");
  });

  it("/kick opens the kick dialog with the reason typed after the name (FR-10.30)", async () => {
    grant(0, { kick: true });
    await typed("/kick Anna spam links");
    render(<PersonActionHost />);
    const dialog = await screen.findByRole("dialog", { name: "Kick Anna? · Local test server" });
    expect(within(dialog).getByLabelText("Reason")).toHaveValue("spam links");
  });

  it("reports the result of a command once its dialog is done (FR-10.35)", async () => {
    grant(0, { kick: true });
    await typed("/kick Anna");
    render(<PersonActionHost />);
    const dialog = await screen.findByRole("dialog", { name: "Kick Anna? · Local test server" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Kick" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(lines()).toEqual(["You kicked Anna from the server. · Only you see this line"]);
  });

  it("/unban names the entry it would remove, then removes it (FR-10.51, AC-10.36)", async () => {
    grant(0, { ban: true });
    expect((await typed("/unban Mallory")).kind).toBe("done");
    const dialog = await asked("Unban Mallory? · Local test server");
    expect(dialog).toHaveTextContent("Reason: spam links");
    fireEvent.click(within(dialog).getByRole("button", { name: "Unban" }));
    await waitFor(() => {
      expect(lines()).toEqual(["Mallory is unbanned. · Only you see this line"]);
    });
    expect(await api.banListOpen(1)).not.toContainEqual(
      expect.objectContaining({ name: "Mallory" }),
    );
    void api.banListClose(1);
  });

  it("/unban with no matching entry says so (FR-10.34)", async () => {
    grant(0, { ban: true });
    expect(await typed("/unban Nobody")).toEqual({
      kind: "notice",
      text: 'No ban matches "Nobody" on this server.',
    });
  });

  it("/moveall asks with the count, then moves everyone in the admin's channel (FR-10.17)", async () => {
    grant(1, { moveUsers: true });
    expect((await typed("/moveall AFK")).kind).toBe("done");
    // The mock holds Zoe, Anna and Ben in Lobby.
    const dialog = await asked("Move 3 people from Lobby to AFK? · Local test server");
    expect(dialog).toHaveTextContent("Everyone in the channel moves, you included.");
    fireEvent.click(within(dialog).getByRole("button", { name: "Move" }));
    await waitFor(() => {
      expect(lines()).toEqual(["Moved 3 of 3 people. · Only you see this line"]);
    });
    const state = getServerState();
    for (const session of [1, ANNA, BEN]) expect(state.users.get(session)?.channelId).toBe(3);
    // Carla was in Raid and stays there: only the admin's own channel moves (FR-10.17).
    expect(state.users.get(4)?.channelId).toBe(2);
  });

  it("refuses /moveall into the channel the admin is already in (FR-10.17)", async () => {
    grant(1, { moveUsers: true });
    expect(await typed("/moveall Lobby")).toEqual({
      kind: "notice",
      text: "You are already in Lobby.",
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(lines()).toEqual([]);
  });
});
