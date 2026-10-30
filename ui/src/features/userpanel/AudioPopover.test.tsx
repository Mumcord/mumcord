import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PreferencesProvider } from "../../app/PreferencesProvider";
import { ViewContext, type View } from "../../app/view";
import { createMockApi, setApi, type MockApi } from "../../api";
import { subscribeAnnouncements } from "../../a11y/announcer";
import "../../i18n";
import { axeViolations } from "../../test/axe";
import { resetServerStore } from "../channels/serverStore";
import { resetNowGamingStore } from "../nowgaming";
import { resetNowPlayingStore } from "../nowplaying";
import { SLIDER_THROTTLE_MS } from "./AudioPopover";
import { UserPanel } from "./UserPanel";
import { NOTICE_MS } from "./useAudioPopover";

const openAudioSettings = vi.fn();
const view = { screen: "main", overlay: null, openAudioSettings } as unknown as View;

let api: MockApi;
let announcements: string[];
let stopAnnouncements: () => void;

async function setup() {
  api = createMockApi({ seed: "ready" });
  setApi(api);
  resetServerStore();
  const [server] = await api.serversList();
  await api.connect((server ?? { id: 0 }).id, null, null);
  render(
    <PreferencesProvider>
      <ViewContext.Provider value={view}>
        <section aria-label="User panel">
          <UserPanel />
        </section>
      </ViewContext.Provider>
    </PreferencesProvider>,
  );
  await screen.findByRole("button", { name: "Mute microphone" });
  await waitFor(() => {
    expect(mute()).toBeEnabled();
  });
}

beforeEach(() => {
  announcements = [];
  stopAnnouncements = subscribeAnnouncements((a) => announcements.push(a.message));
  openAudioSettings.mockReset();
});

afterEach(() => {
  stopAnnouncements();
  setApi(undefined);
  resetServerStore();
  resetNowPlayingStore();
  resetNowGamingStore();
  vi.useRealTimers();
});

const mute = () => screen.getByRole("button", { name: "Mute microphone" });
const deafen = () => screen.getByRole("button", { name: "Deafen" });

async function openInput() {
  fireEvent.contextMenu(mute());
  return screen.findByRole("dialog", { name: "Input device" });
}

async function openOutput() {
  fireEvent.contextMenu(deafen());
  return screen.findByRole("dialog", { name: "Output device" });
}

describe("audio popovers", () => {
  it("opens from right-click, the menu key and the arrow, and left-click still mutes", async () => {
    await setup();
    const muted = vi.spyOn(api, "setSelfMute");
    const dialog = await openInput();
    await waitFor(() => {
      expect(within(dialog).getAllByRole("option")[2]).toHaveFocus();
    });
    expect(muted).not.toHaveBeenCalled();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(mute()).toHaveFocus();

    fireEvent.keyDown(deafen(), { key: "ContextMenu" });
    await screen.findByRole("dialog", { name: "Output device" });
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

    fireEvent.click(screen.getByRole("button", { name: "Microphone options" }));
    await screen.findByRole("dialog", { name: "Input device" });
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

    fireEvent.click(mute());
    expect(muted).toHaveBeenCalledTimes(1);
  });

  it("lists the inputs with the current one checked and the system default marked", async () => {
    await setup();
    const dialog = await openInput();
    const list = within(dialog).getByRole("listbox", { name: "Input device" });
    const options = within(list).getAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual([
      "Built-in Microphone",
      "USB Headset",
      "✓System defaultBuilt-in Microphone",
    ]);
    expect(options.map((o) => o.getAttribute("aria-selected"))).toEqual(["false", "false", "true"]);
  });

  it("switches the input device at once and back to following the system default", async () => {
    await setup();
    const dialog = await openInput();
    fireEvent.click(within(dialog).getByRole("option", { name: /USB Headset/ }));
    await waitFor(async () => {
      expect((await api.settingsGet()).inputDevice).toBe("mic-usb");
    });
    expect(within(dialog).getByRole("option", { name: /USB Headset/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    fireEvent.click(within(dialog).getByRole("option", { name: /System default/ }));
    await waitFor(async () => {
      expect((await api.settingsGet()).inputDevice).toBeNull();
    });
  });

  it("selects a device with the keyboard", async () => {
    await setup();
    const dialog = await openInput();
    const [first, second, systemDefault] = within(dialog).getAllByRole("option");
    await waitFor(() => {
      expect(systemDefault).toHaveFocus();
    });
    fireEvent.keyDown(systemDefault as HTMLElement, { key: "ArrowDown" });
    expect(first).toHaveFocus();
    fireEvent.keyDown(first as HTMLElement, { key: "ArrowDown" });
    expect(second).toHaveFocus();
    fireEvent.keyDown(second as HTMLElement, { key: "Enter" });
    await waitFor(async () => {
      expect((await api.settingsGet()).inputDevice).toBe("mic-usb");
    });
  });

  it("shows the system default as selected when the chosen device is gone", async () => {
    await setup();
    await api.settingsUpdate({ ...(await api.settingsGet()), inputDevice: "unplugged" });
    const dialog = await openInput();
    expect(within(dialog).getByRole("option", { name: /System default/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("shows hot-plugged devices live", async () => {
    await setup();
    const dialog = await openOutput();
    act(() => {
      api.mock.emit("audioDevicesChanged", {
        devices: {
          inputs: [],
          outputs: [
            { id: "out-default", name: "Built-in Speakers", isDefault: true },
            { id: "out-new", name: "Bluetooth Buds", isDefault: false },
          ],
        },
      });
    });
    expect(within(dialog).getByRole("option", { name: /Bluetooth Buds/ })).toBeInTheDocument();
  });

  it("binds the input slider to the setting and the noise suppression switch too", async () => {
    await setup();
    const dialog = await openInput();
    const slider = within(dialog).getByRole("slider", { name: "Volume" });
    expect(slider).toHaveValue("100");
    fireEvent.change(slider, { target: { value: "150" } });
    expect(slider).toHaveValue("150");
    await waitFor(async () => {
      expect((await api.settingsGet()).inputGainPercent).toBe(150);
    });
    const noise = within(dialog).getByRole("switch", { name: "Noise suppression" });
    expect(noise).toBeChecked();
    fireEvent.click(noise);
    await waitFor(async () => {
      expect((await api.settingsGet()).noiseSuppression).toBe(false);
    });
  });

  it("throttles the saves while the slider is dragged but keeps the last value", async () => {
    await setup();
    const dialog = await openOutput();
    const slider = within(dialog).getByRole("slider", { name: "Volume" });
    const update = vi.spyOn(api, "settingsUpdate");
    vi.useFakeTimers();
    fireEvent.change(slider, { target: { value: "80" } });
    fireEvent.change(slider, { target: { value: "60" } });
    fireEvent.change(slider, { target: { value: "40" } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SLIDER_THROTTLE_MS * 2);
    });
    vi.useRealTimers();
    expect(update.mock.calls.length).toBeLessThanOrEqual(2);
    await waitFor(async () => {
      expect((await api.settingsGet()).outputVolumePercent).toBe(40);
    });
  });

  it("plays the test sound and opens the voice settings from the output popover", async () => {
    await setup();
    const test = vi.spyOn(api, "audioTestSound");
    const dialog = await openOutput();
    fireEvent.click(within(dialog).getByRole("button", { name: "Test sound" }));
    expect(test).toHaveBeenCalledTimes(1);
    fireEvent.click(within(dialog).getByRole("button", { name: "Voice settings…" }));
    expect(openAudioSettings).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("hides the level meter from screen readers and has no output-only controls on the input", async () => {
    await setup();
    const dialog = await openInput();
    expect(dialog.querySelector(".audio-popover__level")).toHaveAttribute("aria-hidden", "true");
    expect(within(dialog).queryByRole("button", { name: "Test sound" })).not.toBeInTheDocument();
    expect(await axeViolations(dialog)).toEqual([]);
  });

  it("tells the user briefly when a device fell back to the system default", async () => {
    await setup();
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    act(() => {
      api.mock.emit("audioDeviceFallback", { input: true });
    });
    expect(screen.getByText("Microphone unplugged. Using the system default.")).toBeInTheDocument();
    expect(announcements).toContain("Microphone unplugged. Using the system default.");
    act(() => {
      vi.advanceTimersByTime(NOTICE_MS);
    });
    expect(
      screen.queryByText("Microphone unplugged. Using the system default."),
    ).not.toBeInTheDocument();
  });

  it("closes when the focus leaves the popover", async () => {
    await setup();
    const dialog = await openInput();
    fireEvent.blur(within(dialog).getAllByRole("option")[0] as HTMLElement, {
      relatedTarget: document.body,
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
