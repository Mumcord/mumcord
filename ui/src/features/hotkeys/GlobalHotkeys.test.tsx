import { fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type ChannelInfo, type MockApi } from "../../api";
import type { ServerState } from "../channels/serverStore";
import { GlobalHotkeys } from "./GlobalHotkeys";
import { onUiHotkey } from "./hotkeyBus";
import { canonicalCombo } from "./hotkeyKeys";
import { setHotkeySettings } from "./hotkeyStore";
import { channelAt, channelPath } from "./runHotkey";

let api: MockApi;

beforeEach(() => {
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  setHotkeySettings({ globalHotkeys: true, hotkeys: [], pttKey: null, cameraHotkey: null });
  setApi(undefined);
});

describe("global hotkeys (#64)", () => {
  it("runs the action of a hotkey the engine reports", async () => {
    const dndSet = vi.spyOn(api, "dndSet");
    const toggleColumn = vi.fn();
    const off = onUiHotkey("toggleMemberColumn", toggleColumn);
    render(<GlobalHotkeys overlayOpen={false} />);
    api.mock.emit("hotkeyPressed", { action: { kind: "toggleDnd" } });
    await waitFor(() => {
      expect(dndSet).toHaveBeenCalledWith(true);
    });
    api.mock.emit("hotkeyPressed", { action: { kind: "toggleMemberColumn" } });
    expect(toggleColumn).toHaveBeenCalledTimes(1);
    off();
  });

  it("works in the window only while background hotkeys are off, never while typing", async () => {
    const dndSet = vi.spyOn(api, "dndSet");
    setHotkeySettings({
      globalHotkeys: false,
      hotkeys: [{ key: "Ctrl+F7", action: { kind: "toggleDnd" } }],
      pttKey: null,
      cameraHotkey: null,
    });
    const { container, rerender } = render(
      <>
        <GlobalHotkeys overlayOpen={false} />
        <input aria-label="Message" />
      </>,
    );
    const input = container.querySelector("input");
    if (input === null) throw new Error("no input");
    fireEvent.keyDown(input, { code: "F7", key: "F7", ctrlKey: true });
    expect(dndSet).not.toHaveBeenCalled();

    fireEvent.keyDown(document.body, { code: "F7", key: "F7", ctrlKey: true });
    await waitFor(() => {
      expect(dndSet).toHaveBeenCalledTimes(1);
    });

    rerender(<GlobalHotkeys overlayOpen />);
    fireEvent.keyDown(document.body, { code: "F7", key: "F7", ctrlKey: true });
    expect(dndSet).toHaveBeenCalledTimes(1);
  });

  it("leaves the window keys alone while the engine handles them", () => {
    const dndSet = vi.spyOn(api, "dndSet");
    setHotkeySettings({
      globalHotkeys: true,
      hotkeys: [{ key: "Ctrl+F7", action: { kind: "toggleDnd" } }],
      pttKey: null,
      cameraHotkey: null,
    });
    render(<GlobalHotkeys overlayOpen={false} />);
    fireEvent.keyDown(document.body, { code: "F7", key: "F7", ctrlKey: true });
    expect(dndSet).not.toHaveBeenCalled();
  });

  it("runs a default in the window while the engine holds the user's own keys (#584)", async () => {
    const toggleColumn = vi.fn();
    const dndSet = vi.spyOn(api, "dndSet");
    const off = onUiHotkey("toggleMemberColumn", toggleColumn);
    setHotkeySettings({
      globalHotkeys: true,
      hotkeys: [{ key: "Ctrl+F7", action: { kind: "toggleDnd" } }],
      pttKey: null,
      cameraHotkey: null,
    });
    render(<GlobalHotkeys overlayOpen={false} />);
    fireEvent.keyDown(document.body, { code: "KeyU", key: "u", ctrlKey: true, shiftKey: true });
    await waitFor(() => {
      expect(toggleColumn).toHaveBeenCalledTimes(1);
    });
    // The user's own key stays with the engine: pressed here, it does nothing twice.
    fireEvent.keyDown(document.body, { code: "F7", key: "F7", ctrlKey: true });
    expect(dndSet).not.toHaveBeenCalled();
    off();
  });

  it("runs only defaults that no other key uses (#584)", async () => {
    const toggleColumn = vi.fn();
    const dndSet = vi.spyOn(api, "dndSet");
    const off = onUiHotkey("toggleMemberColumn", toggleColumn);
    // Ctrl+Shift+U is the member column's default, but the user gave it to Do Not Disturb.
    setHotkeySettings({
      globalHotkeys: false,
      hotkeys: [{ key: "Ctrl+Shift+U", action: { kind: "toggleDnd" } }],
      pttKey: null,
      cameraHotkey: null,
    });
    render(<GlobalHotkeys overlayOpen={false} />);
    fireEvent.keyDown(document.body, { code: "KeyU", key: "u", ctrlKey: true, shiftKey: true });
    await waitFor(() => {
      expect(dndSet).toHaveBeenCalledTimes(1);
    });
    expect(toggleColumn).not.toHaveBeenCalled();
    off();
  });

  it("hands the camera default to the engine's camera (#584)", async () => {
    const view = vi.spyOn(api, "cameraView");
    setHotkeySettings({
      globalHotkeys: true,
      hotkeys: [],
      pttKey: null,
      cameraHotkey: null,
    });
    render(<GlobalHotkeys overlayOpen={false} />);
    fireEvent.keyDown(document.body, { code: "KeyC", key: "c", ctrlKey: true, shiftKey: true });
    await waitFor(() => {
      expect(view).toHaveBeenCalled();
    });
  });

  it("turns key presses into the stored key form", () => {
    expect(canonicalCombo("Ctrl+Shift+KeyD")).toBe("Ctrl+Shift+D");
    expect(canonicalCombo("Alt+Digit1")).toBe("Alt+1");
    expect(canonicalCombo("F9")).toBe("F9");
    expect(canonicalCombo("Ctrl+Numpad1")).toBe("Ctrl+Numpad1");
  });

  it("finds channels by their name path and back", () => {
    const channel = (id: number, parentId: number | null, name: string) =>
      ({ id, parentId, name }) as ChannelInfo;
    const channels = new Map(
      [channel(0, null, "Root"), channel(1, 0, "Night Owls"), channel(2, 1, "Lobby")].map(
        (c) => [c.id, c] as const,
      ),
    );
    // Only the channel tree matters for the path lookup.
    const state = { channels } as unknown as ServerState;
    expect(channelPath(state, 2)).toEqual(["Night Owls", "Lobby"]);
    expect(channelAt(state, ["Night Owls", "Lobby"])).toBe(2);
    expect(channelAt(state, [])).toBe(0);
    expect(channelAt(state, ["Missing"])).toBeNull();
  });
});
