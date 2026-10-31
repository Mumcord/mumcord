import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../../api";
import { PreferencesContext, type Preferences } from "../../../app/preferences";
import { fakeView } from "../../../app/testView";
import { ViewContext } from "../../../app/view";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { SettingsScreen } from "../SettingsScreen";
import { RECORD_TIMEOUT_MS } from "./HotkeyRow";

let api: MockApi;

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  setApi(undefined);
});

async function openHotkeys() {
  const preferences: Preferences = {
    theme: "system",
    resolvedTheme: "dark",
    language: "en",
    chatDisplay: "cozy",
    linkPreviews: true,
    hideEmptyChannels: false,
    talkingFirst: false,
    showWeakConnectionIcon: true,
    lowLatency: false,
    applySettings: vi.fn(),
  };
  const view = render(
    <PreferencesContext.Provider value={preferences}>
      <ViewContext.Provider value={fakeView()}>
        <main aria-label="Settings test">
          <SettingsScreen />
        </main>
      </ViewContext.Provider>
    </PreferencesContext.Provider>,
  );
  fireEvent.click(await screen.findByRole("tab", { name: "Hotkeys" }));
  await screen.findByRole("heading", { name: "Actions" });
  return view;
}

/** The key field of an action; its accessible name is the action plus the key. */
function field(action: string) {
  return screen.getByRole("button", { name: new RegExp(`^${action}`) });
}

function press(target: HTMLElement, code: string, modifiers: { ctrlKey?: boolean } = {}) {
  fireEvent.keyDown(target, { code, key: code, ...modifiers });
}

function record(action: string, code: string, modifiers: { ctrlKey?: boolean } = {}) {
  const button = field(action);
  fireEvent.click(button);
  expect(button).toHaveTextContent("Press a key combination…");
  press(button, code, modifiers);
}

describe("Settings → Hotkeys (#64)", () => {
  it("lists every action without a key and works in the background by default", async () => {
    const { container } = await openHotkeys();
    for (const action of [
      "Push-to-talk",
      "Mute / unmute microphone",
      "Deafen / undeafen",
      "Toggle Do Not Disturb",
      "Join / leave voice",
      "Switch voice server",
      "Back to previous channel",
      "Show / hide member column",
      "Open quick settings",
      "Open Mumcord",
    ]) {
      expect(field(action)).toHaveTextContent("Not set");
    }
    expect(
      screen.getByRole("switch", { name: /Work while Mumcord is in the background/ }),
    ).toBeChecked();
    expect(screen.getByRole("button", { name: 'Add "switch to channel" shortcut…' })).toBeVisible();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("records a key combination and saves it", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openHotkeys();
    record("Mute / unmute microphone", "F7", { ctrlKey: true });
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(
        expect.objectContaining({ hotkeys: [{ key: "Ctrl+F7", action: { kind: "toggleMute" } }] }),
      );
    });
    await waitFor(() => {
      expect(field("Mute / unmute microphone")).toHaveAccessibleName(
        "Mute / unmute microphone Ctrl + F7",
      );
    });
    fireEvent.click(screen.getByRole("button", { name: "Clear Mute / unmute microphone" }));
    await waitFor(() => {
      expect(field("Mute / unmute microphone")).toHaveTextContent("Not set");
    });
  });

  it("shows a conflict and can assign the key here, clearing the other", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openHotkeys();
    record("Mute / unmute microphone", "F7", { ctrlKey: true });
    await waitFor(() => {
      expect(update).toHaveBeenCalledTimes(1);
    });
    record("Deafen / undeafen", "F7", { ctrlKey: true });
    const alert = (await screen.findByText("Conflict.")).closest<HTMLElement>("[role='alert']");
    if (alert === null) throw new Error("conflict is not announced");
    expect(alert).toHaveTextContent(
      'Conflict. This combination is already used by "Mute / unmute microphone".',
    );
    expect(update).toHaveBeenCalledTimes(1);
    fireEvent.click(within(alert).getByRole("button", { name: "Assign here" }));
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(
        expect.objectContaining({
          hotkeys: [{ key: "Ctrl+F7", action: { kind: "toggleDeafen" } }],
        }),
      );
    });
  });

  it("refuses Mumcord's own shortcuts without an assign button", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openHotkeys();
    const button = field("Toggle Do Not Disturb");
    fireEvent.click(button);
    fireEvent.keyDown(button, { code: "KeyM", key: "M", ctrlKey: true, shiftKey: true });
    const alert = await screen.findByText(/own shortcut for showing and hiding the member column/);
    expect(alert.closest("[role='alert']")).not.toBeNull();
    expect(screen.queryByRole("button", { name: "Assign here" })).toBeNull();
    expect(update).not.toHaveBeenCalled();
  });

  it("Esc cancels, Backspace clears, and recording stops after five seconds", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    await openHotkeys();
    const button = field("Open Mumcord");
    fireEvent.click(button);
    press(button, "Escape");
    expect(button).toHaveTextContent("Not set");

    fireEvent.click(button);
    expect(screen.getByText(/Recording stops after 5 seconds/)).toBeVisible();
    act(() => {
      vi.advanceTimersByTime(RECORD_TIMEOUT_MS);
    });
    expect(button).toHaveTextContent("Not set");
  });

  it("resets all hotkeys", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openHotkeys();
    record("Open Mumcord", "F8", { ctrlKey: true });
    const reset = screen.getByRole("button", { name: "Reset all hotkeys" });
    await waitFor(() => {
      expect(reset).toBeEnabled();
    });
    fireEvent.click(reset);
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ hotkeys: [] }));
    });
  });

  it("explains the foreground-only mode when switched off", async () => {
    await openHotkeys();
    fireEvent.click(
      screen.getByRole("switch", { name: /Work while Mumcord is in the background/ }),
    );
    expect(
      await screen.findByText("Shortcuts only work while the Mumcord window is in front."),
    ).toBeVisible();
  });
});
