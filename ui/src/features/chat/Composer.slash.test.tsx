import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Conversation, PluginCommand } from "../../api";
import type { MockApi } from "../../api/mock";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll } from "../channels/testUtils";
import { axeViolations } from "../../test/axe";
import { privateLinesFor, resetPrivateLines } from "./privateLines";
import { Composer } from "./Composer";

/**
 * The command list of the chat box (Epic 4, issue #378): Mumcord's own admin commands above the
 * plugins', only the ones the server's rights allow, and a line that runs one never reaches the
 * chat as a message.
 */
const LIMITS = { text: 5000, image: 100_000, allowHtml: true };
const CHANNEL: Conversation = { kind: "channel", channelId: 1 };
const ROLL: PluginCommand = {
  pluginId: "dice",
  pluginName: "Dice roller",
  name: "roll",
  description: "Roll dice",
  usage: "/roll 2d6",
  takenBy: null,
};

let api: MockApi;

function renderComposer(): HTMLTextAreaElement {
  render(
    <main>
      <Composer
        target={{ kind: "channel", channelId: 1 }}
        conversation={CHANNEL}
        limits={LIMITS}
        label="Message #Lobby"
        blockedReason={null}
      />
    </main>,
  );
  return screen.getByRole<HTMLTextAreaElement>("textbox", { name: "Message #Lobby" });
}

function grant(channelId: number, permissions: Record<string, boolean>): void {
  const channel = getServerState().channels.get(channelId);
  if (!channel) throw new Error(`the mock has no channel ${String(channelId)}`);
  api.mock.updateChannel(channelId, {
    ...channel,
    permissions: { ...channel.permissions, ...permissions },
  });
}

/** Types into the box, so the caret is where a real user would have it. */
function type(input: HTMLTextAreaElement, text: string): void {
  fireEvent.change(input, { target: { value: text } });
  input.focus();
}

const rows = (): HTMLElement[] => [...screen.getAllByRole("option")];

beforeEach(async () => {
  api = await connectedMock({ bindStore: true });
  resetPrivateLines();
  api.pluginCommands = () => Promise.resolve([ROLL]);
});

afterEach(() => {
  resetPrivateLines();
  resetAll();
});

describe("the command list (FR-10.30, FR-10.32)", () => {
  it("shows the built-in commands first, marked Built-in, then the plugins' (mockup 01)", async () => {
    grant(1, { muteDeafen: true });
    const input = renderComposer();
    type(input, "/");
    const list = await screen.findByRole("listbox", { name: "Slash commands" });
    const keywords = rows().map((row) => row.textContent);
    expect(list.textContent).toContain("Admin commands · built-in");
    expect(keywords[0]).toContain("/mute");
    expect(keywords[0]).toContain("Built-in");
    expect(keywords.at(-1)).toContain("/roll");
    expect(keywords.at(-1)).toContain("Dice roller");
    // The box points at the list and its active row, so the keyboard stays in the box
    // (issue #378, a11y); it stays a plain textbox so screen readers announce it correctly.
    expect(input).not.toHaveAttribute("role");
    expect(input.getAttribute("aria-controls")).toBe(list.id);
    // Escape closes the list first: the box itself has to stay clean.
    fireEvent.keyDown(input, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("listbox")).toBeNull();
    });
    expect(await axeViolations(document.body)).toEqual([]);
  });

  it("shows no admin command to a user without a right (FR-10.3)", async () => {
    const input = renderComposer();
    type(input, "/");
    const list = await screen.findByRole("listbox", { name: "Slash commands" });
    expect(list.textContent).not.toContain("/mute");
    expect(list.textContent).toContain("/roll");
  });

  it("completes the highlighted command with Tab", async () => {
    grant(0, { ban: true });
    const input = renderComposer();
    type(input, "/b");
    await screen.findByRole("option");
    fireEvent.keyDown(input, { key: "Tab" });
    expect(input.value).toBe("/ban ");
  });

  it("suggests the people a command may name, with their channel (mockup 02)", async () => {
    grant(1, { muteDeafen: true });
    const input = renderComposer();
    type(input, "/mute An");
    await screen.findByRole("option", { name: /Anna/ });
    const anna = screen.getByRole("option", { name: /Anna/ });
    expect(anna.textContent).toContain("Lobby");
    // The argument rows are under their own heading, not under the commands.
    expect(rows().some((row) => row.textContent.includes("/mute"))).toBe(false);
    fireEvent.click(anna);
    await waitFor(() => {
      expect(input.value).toBe("/mute Anna ");
    });
  });

  it("closes on Escape and comes back when the word changes", async () => {
    grant(1, { muteDeafen: true });
    const input = renderComposer();
    type(input, "/mute");
    await screen.findByRole("listbox", { name: "Slash commands" });
    fireEvent.keyDown(input, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("listbox")).toBeNull();
    });
    type(input, "/mute ");
    await screen.findByRole("option", { name: /Anna/ });
  });
});

describe("running a command from the box (FR-10.33 … FR-10.35)", () => {
  it("runs the line, clears the box and writes the result line to the chat only its admin sees", async () => {
    grant(1, { muteDeafen: true });
    const mute = vi.spyOn(api, "personSetServerMute");
    const send = vi.spyOn(api, "sendChannelMessage");
    const input = renderComposer();
    type(input, "/mute Anna");
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => {
      expect(mute).toHaveBeenCalledWith(1, 2, true);
    });
    expect(input.value).toBe("");
    expect(send).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(privateLinesFor(1, CHANNEL).map((line) => line.message.body)).toEqual([
        "You muted Anna on the server.",
      ]);
    });
    expect(privateLinesFor(1, CHANNEL)[0]?.note).toBe("Only you see this line");
  });

  it("takes a leading @ like a mention and still runs the command", async () => {
    grant(1, { muteDeafen: true });
    const mute = vi.spyOn(api, "personSetServerMute");
    const send = vi.spyOn(api, "sendChannelMessage");
    const input = renderComposer();
    type(input, "/mute @Anna");
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => {
      expect(mute).toHaveBeenCalledWith(1, 2, true);
    });
    expect(input.value).toBe("");
    expect(send).not.toHaveBeenCalled();
  });

  it("autocompletes a name written with @ into the bare name", async () => {
    grant(1, { muteDeafen: true });
    const input = renderComposer();
    type(input, "/mute @An");
    const list = await screen.findByRole("listbox", { name: "Slash commands" });
    expect(list.textContent).toContain("People");
    expect(rows().some((row) => row.textContent.includes("Anna"))).toBe(true);
    // Completing writes the name without the @, so Enter runs it right away.
    fireEvent.keyDown(input, { key: "Tab" });
    expect(input.value).toBe("/mute Anna ");
  });

  it("keeps a command the admin may not use in the box and sends nothing (FR-10.33, Q1.10)", async () => {
    // No kick right on the root channel: the command is not listed, not completed and not run.
    const kick = vi.spyOn(api, "personKick");
    const send = vi.spyOn(api, "sendChannelMessage");
    // A moderator in Lobby: the toggles are offered, /kick is not, because the root channel
    // does not grant Kick (FR-10.32).
    grant(1, { muteDeafen: true });
    const input = renderComposer();
    type(input, "/");
    await screen.findByRole("listbox", { name: "Slash commands" });
    expect(rows().some((row) => row.textContent.includes("/mute"))).toBe(true);
    expect(rows().some((row) => row.textContent.includes("/kick"))).toBe(false);
    type(input, "/kick Anna");
    fireEvent.keyDown(input, { key: "Enter" });
    await act(() => Promise.resolve());
    expect(input.value).toBe("/kick Anna");
    expect(kick).not.toHaveBeenCalled();
    expect(send).not.toHaveBeenCalled();
  });

  it("shows why a name did not match, above the box (mockup 04b)", async () => {
    grant(1, { muteDeafen: true });
    const input = renderComposer();
    type(input, "/mute Annaa");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(await screen.findByRole("alert")).toHaveTextContent(
      'No one named "Annaa" is on this server right now. Did you mean Anna?',
    );
    expect(privateLinesFor(1, CHANNEL)).toEqual([]);
  });

  it("asks which person was meant, and runs the one the admin picks (mockup 04c)", async () => {
    grant(1, { muteDeafen: true });
    const anna = getServerState().users.get(2);
    if (!anna) throw new Error("the mock has no Anna");
    api.mock.upsertUser({ ...anna, session: 20, name: "Annabel" });
    const mute = vi.spyOn(api, "personSetServerMute");
    const input = renderComposer();
    type(input, "/mute Ann");
    // Escape first, so Enter runs the whole line instead of completing the highlighted name.
    fireEvent.keyDown(input, { key: "Escape" });
    fireEvent.keyDown(input, { key: "Enter" });
    const choice = await screen.findByRole("option", { name: /Annabel/ });
    expect(rows()).toHaveLength(2);
    fireEvent.click(choice);
    await waitFor(() => {
      expect(mute).toHaveBeenCalledWith(1, 20, true);
    });
  });
});
