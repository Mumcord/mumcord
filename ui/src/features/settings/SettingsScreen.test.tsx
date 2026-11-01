import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { hintPreferences, setHintPreference } from "../../a11y/hintPreferences";
import { createMockApi, setApi, type MockApi } from "../../api";
import { PreferencesContext, type Preferences } from "../../app/preferences";
import { fakeView } from "../../app/testView";
import { ViewContext, type View } from "../../app/view";
import { setSettingsOpener } from "./settingsOpener";
import { applyLanguage } from "../../i18n";
import { openInBrowser } from "../chat/openLink";
import { chooseOption, optionLabels } from "../../components/SelectTestUtils";
import { axeViolations } from "../../test/axe";
import { applyThemeToDocument } from "../../theme/theme";
import { pickOpenPath, pickSavePath } from "./fileDialogs";
import { SettingsScreen } from "./SettingsScreen";

vi.mock("../chat/openLink", () => ({ openInBrowser: vi.fn() }));
vi.mock("./fileDialogs", () => ({
  pickSavePath: vi.fn(),
  pickOpenPath: vi.fn(),
}));

const savePath = vi.mocked(pickSavePath);
const openPath = vi.mocked(pickOpenPath);

let api: MockApi;
let applySettings: Mock<Preferences["applySettings"]>;
let view: View;

async function setup(theme: "dark" | "light" = "dark") {
  await applyLanguage("en");
  applyThemeToDocument(theme);
  api = createMockApi();
  setApi(api);
  applySettings = vi.fn<Preferences["applySettings"]>();
  view = fakeView();
}

beforeEach(async () => {
  await setup();
  savePath.mockReset();
  openPath.mockReset();
  vi.stubGlobal(
    "fetch",
    vi.fn(() => Promise.resolve(new Response("Some library — Apache-2.0", { status: 200 }))),
  );
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  setApi(undefined);
});

function renderScreen() {
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
    applySettings,
  };
  return render(
    <PreferencesContext.Provider value={preferences}>
      <ViewContext.Provider value={view}>
        <main aria-label="Settings test">
          <h1>Settings</h1>
          <SettingsScreen />
        </main>
      </ViewContext.Provider>
    </PreferencesContext.Provider>,
  );
}

/** The explanation behind the "?" next to a control (#155). */
function expectTip(control: HTMLElement, text: string | RegExp) {
  const row = control.closest<HTMLElement>(".settings-switch, .settings-control-row");
  const tip = row?.querySelector(".info-tip");
  expect(tip?.getAttribute("data-tooltip")).toEqual(
    typeof text === "string" ? text : expect.stringMatching(text),
  );
}

/** Switches to another group in the screen that is already rendered. */
async function switchSection(name: string) {
  fireEvent.click(screen.getByRole("tab", { name }));
  await waitFor(() => {
    expect(screen.queryByText("Loading settings…")).not.toBeInTheDocument();
  });
}

async function openSection(name: string) {
  renderScreen();
  fireEvent.click(await screen.findByRole("tab", { name }));
  // Wait until the settings record has loaded.
  await waitFor(() => {
    expect(screen.queryByText("Loading settings…")).not.toBeInTheDocument();
  });
}

const GROUPS = [
  "General",
  "Account & Identity",
  "Voice & Audio",
  "Chat & Notifications",
  "Privacy & Connections",
  "Appearance",
  "Servers",
  "Gaming & Overlay",
  "Advanced & Developer",
  "About",
];

describe("SettingsScreen", () => {
  it("lists the groups as a vertical tab list operated with the arrow keys", () => {
    renderScreen();
    const list = screen.getByRole("tablist", { name: "Settings groups" });
    expect(list).toHaveAttribute("aria-orientation", "vertical");
    expect(
      within(list)
        .getAllByRole("tab")
        .map((tab) => tab.textContent),
    ).toEqual(GROUPS);
    const identity = screen.getByRole("tab", { name: "General" });
    expect(identity).toHaveAttribute("aria-selected", "true");
    expect(identity).toHaveAttribute("tabindex", "0");
    expect(screen.getByRole("heading", { level: 2, name: "General" })).toBeInTheDocument();

    fireEvent.keyDown(identity, { key: "ArrowDown" });
    const servers = screen.getByRole("tab", { name: "Account & Identity" });
    expect(servers).toHaveFocus();
    expect(servers).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel", { name: "Account & Identity" })).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { level: 2, name: "Account & Identity" }),
    ).toBeInTheDocument();

    fireEvent.keyDown(servers, { key: "End" });
    expect(screen.getByRole("tab", { name: "About" })).toHaveFocus();
    fireEvent.keyDown(screen.getByRole("tab", { name: "About" }), { key: "ArrowDown" });
    expect(identity).toHaveFocus();
    fireEvent.keyDown(identity, { key: "ArrowUp" });
    expect(screen.getByRole("tab", { name: "About" })).toHaveFocus();
    fireEvent.keyDown(screen.getByRole("tab", { name: "About" }), { key: "Home" });
    expect(identity).toHaveFocus();
  });

  it.each(["dark", "light"] as const)("has no axe violations in the %s theme", async (theme) => {
    await setup(theme);
    const { container } = renderScreen();
    for (const name of GROUPS) {
      fireEvent.click(screen.getByRole("tab", { name }));
      await waitFor(() => {
        expect(screen.queryByText("Loading settings…")).not.toBeInTheDocument();
      });
      expect(await axeViolations(container)).toEqual([]);
    }
  });

  it("saves a change immediately and applies theme and language", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Appearance");
    chooseOption(screen.getByRole("combobox", { name: "Dark or light" }), "Light");
    fireEvent.click(screen.getByRole("button", { name: "Forest, theme" }));
    const light = { theme: "light", appThemeId: "preset:forest" };
    await waitFor(() => {
      expect(applySettings).toHaveBeenCalledWith(expect.objectContaining(light));
    });
    expect(update).toHaveBeenCalledWith(expect.objectContaining(light));

    await switchSection("General");
    fireEvent.click(screen.getByRole("radio", { name: "Minimize to the tray" }));
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(
        expect.objectContaining({ ...light, closeBehaviour: "minimizeToTray" }),
      );
    });
  });

  it("switches the message display between cozy and compact and applies it", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Appearance");
    const cozy = screen.getByRole("radio", { name: /^Cozy/ });
    expect(cozy).toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: /^Compact/ }));
    await waitFor(() => {
      expect(applySettings).toHaveBeenCalledWith(
        expect.objectContaining({ chatDisplay: "compact" }),
      );
    });
    expect(update).toHaveBeenCalledWith(expect.objectContaining({ chatDisplay: "compact" }));
  });

  it("offers start at login with start minimized enabled only while it is on", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("General");
    const autostart = screen.getByRole("switch", { name: "Start Mumcord when I log in" });
    const minimized = screen.getByRole("switch", { name: "Start minimized" });
    expect(autostart).not.toBeChecked();
    expect(minimized).toBeChecked();
    expect(minimized).toBeDisabled();
    expectTip(
      minimized,
      "When Mumcord starts at login, it waits in the tray instead of opening the window.",
    );

    fireEvent.click(autostart);
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ autostart: true }));
    });
    expect(minimized).toBeEnabled();
    fireEvent.click(minimized);
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(
        expect.objectContaining({ autostart: true, startMinimized: false }),
      );
    });
  });

  it("lets the user turn off the read-only connect", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Voice & Audio");
    const toggle = screen.getByRole("switch", { name: "Connect muted until I join a channel" });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ connectMuted: false }));
    });
  });

  it("switches link previews off and applies it to the chat right away", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Privacy & Connections");
    const toggle = screen.getByRole("switch", { name: "Show link previews and embeds" });
    expect(toggle).toBeChecked();
    expectTip(toggle, /see your IP address/);
    fireEvent.click(toggle);
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ linkPreviews: false }));
    });
    await waitFor(() => {
      expect(applySettings).toHaveBeenLastCalledWith(
        expect.objectContaining({ linkPreviews: false }),
      );
    });
  });

  it("rolls a failed save back and explains it", async () => {
    await openSection("General");
    api.mock.failNext("settingsUpdate", "io");
    fireEvent.click(screen.getByRole("radio", { name: "Deutsch" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The last change could not be saved",
    );
    expect(screen.getByRole("radio", { name: "Follow system" })).toBeChecked();
    expect(applySettings).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("radio", { name: "Español" }));
    await waitFor(() => {
      expect(applySettings).toHaveBeenCalledWith(expect.objectContaining({ language: "es" }));
    });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("stores null for the system language", async () => {
    await api.settingsUpdate({ ...(await api.settingsGet()), language: "de" });
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("General");
    fireEvent.click(screen.getByRole("radio", { name: "Follow system" }));
    await waitFor(() => {
      expect(update).toHaveBeenCalledWith(expect.objectContaining({ language: null }));
    });
  });

  it("toggles notification sounds", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Voice & Audio");
    const toggle = screen.getByRole("switch", { name: "You mute or unmute yourself" });
    fireEvent.click(toggle);
    await waitFor(() => {
      expect(update).toHaveBeenCalledWith(
        expect.objectContaining({
          sounds: {
            joinLeave: true,
            selfMute: false,
            selfDeaf: true,
            messages: true,
            mention: true,
            mentionNotify: true,
            voiceJoin: true,
            hangUp: true,
            partyStarted: true,
            partyJoined: true,
            partyLeft: true,
            shareStarted: true,
            shareStopped: true,
            viewerStarted: true,
            viewerStopped: true,
            kicked: true,
            banned: true,
          },
        }),
      );
    });
  });

  it("has separate mention sound and notification switches, both on by default", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Voice & Audio");
    const sound = screen.getByRole("switch", { name: "Sound when mentioned" });
    const notify = screen.getByRole("switch", { name: "Notify when mentioned" });
    expect(sound).toBeChecked();
    expect(notify).toBeChecked();
    fireEvent.click(notify);
    await waitFor(() => {
      expect(update.mock.lastCall?.[0].sounds).toMatchObject({
        mention: true,
        mentionNotify: false,
      });
    });
  });

  it("toggles the own join and hang-up sounds separately", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Voice & Audio");
    const join = screen.getByRole("switch", { name: "You join a channel for voice" });
    const hangUp = screen.getByRole("switch", { name: "You disconnect from a server" });
    expect(join).toBeChecked();
    expect(hangUp).toBeChecked();
    fireEvent.click(hangUp);
    await waitFor(() => {
      expect(update.mock.lastCall?.[0].sounds).toMatchObject({ voiceJoin: true, hangUp: false });
    });
    fireEvent.click(screen.getByRole("switch", { name: "You join a channel for voice" }));
    await waitFor(() => {
      expect(update.mock.lastCall?.[0].sounds).toMatchObject({ voiceJoin: false, hangUp: false });
    });
  });

  it("toggles the kicked and banned sounds separately, both on by default", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Voice & Audio");
    const kicked = screen.getByRole("switch", { name: "You are kicked from a server" });
    const banned = screen.getByRole("switch", { name: "You are banned from a server" });
    expect(kicked).toBeChecked();
    expect(banned).toBeChecked();
    fireEvent.click(banned);
    await waitFor(() => {
      expect(update.mock.lastCall?.[0].sounds).toMatchObject({ kicked: true, banned: false });
    });
    fireEvent.click(screen.getByRole("switch", { name: "You are kicked from a server" }));
    await waitFor(() => {
      expect(update.mock.lastCall?.[0].sounds).toMatchObject({ kicked: false, banned: false });
    });
  });

  it("chooses how message notifications are shown", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Chat & Notifications");
    const group = screen.getByRole("group", { name: "Notifications for new messages" });
    expect(within(group).getByRole("radio", { name: /^Both/ })).toBeChecked();
    fireEvent.click(within(group).getByRole("radio", { name: "Off" }));
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ notifications: "off" }));
    });
    fireEvent.click(within(group).getByRole("radio", { name: /^In-app/ }));
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ notifications: "inApp" }));
    });
  });

  it("shows the overlay section", async () => {
    await openSection("Gaming & Overlay");
    expect(await screen.findByRole("heading", { name: "In-game overlay" })).toBeInTheDocument();
  });

  it("offers the in-game overlay through the engine", async () => {
    const setMode = vi.spyOn(api, "ingameOverlaySetMode");
    await openSection("Gaming & Overlay");
    fireEvent.click(await screen.findByRole("radio", { name: "On" }));
    expect(setMode).toHaveBeenCalledWith("on");
    expect(screen.queryByText(/not available on this system/)).not.toBeInTheDocument();
  });
});

describe("Audio section", () => {
  it("lists devices, saves a choice and stores null for the system default", async () => {
    vi.spyOn(api, "audioDevices").mockResolvedValue({
      inputs: [
        {
          id: "default",
          name: "Default ALSA Input (currently PipeWire Media Server)",
          isDefault: true,
        },
        { id: "mic-usb", name: "USB Headset", isDefault: false },
      ],
      outputs: [],
    });
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Voice & Audio");
    const mic = await screen.findByRole("combobox", { name: "Microphone" });
    await waitFor(() => {
      expect(optionLabels(mic)).toContain("Default ALSA Input");
    });
    expect(mic).toHaveTextContent("System default");
    chooseOption(mic, "USB Headset");
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ inputDevice: "mic-usb" }));
    });
    chooseOption(mic, "System default");
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ inputDevice: null }));
    });
  });

  it("follows hot-plugged devices", async () => {
    await openSection("Voice & Audio");
    const mic = await screen.findByRole("combobox", { name: "Microphone" });
    await waitFor(() => {
      expect(optionLabels(mic)).toContain("USB Headset");
    });
    act(() => {
      api.mock.emit("audioDevicesChanged", {
        devices: { inputs: [{ id: "new", name: "Webcam Mic", isDefault: false }], outputs: [] },
      });
    });
    expect(optionLabels(mic)).toContain("Webcam Mic");
  });

  it("runs the microphone test through the engine's phases with a level meter", async () => {
    const start = vi.spyOn(api, "micTestStart").mockResolvedValue(null);
    const stop = vi.spyOn(api, "micTestStop");
    await openSection("Voice & Audio");
    vi.useFakeTimers();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Start test" }));
      await Promise.resolve();
    });
    expect(start).toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: "More information: Microphone test" }).dataset.tooltip,
    ).toMatch(/played back to you/);
    act(() => {
      api.mock.emit("micLevel", { level: 0.5 });
    });
    // The microphone test card on the same page shows the same level.
    for (const meter of screen.getAllByRole("meter", { name: "Microphone level" })) {
      expect(meter).toHaveAttribute("aria-valuenow", "50");
    }
    act(() => {
      api.mock.emit("micTestPhaseChanged", { phase: "playing" });
    });
    expect(screen.getByText("Playing back your voice…")).toBeInTheDocument();
    act(() => {
      api.mock.emit("micTestPhaseChanged", { phase: "done" });
    });
    expect(stop).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Start test" })).toBeInTheDocument();
  });

  it("stops the microphone test when leaving the section", async () => {
    vi.spyOn(api, "micTestStart").mockResolvedValue(null);
    const stop = vi.spyOn(api, "micTestStop");
    await openSection("Voice & Audio");
    fireEvent.click(screen.getByRole("button", { name: "Start test" }));
    await screen.findByRole("button", { name: "Stop recording" });
    fireEvent.click(screen.getByRole("tab", { name: "General" }));
    expect(stop).toHaveBeenCalledTimes(1);
  });
});

describe("Voice section", () => {
  async function openPushToTalk() {
    await openSection("Voice & Audio");
    fireEvent.click(await screen.findByRole("radio", { name: /Push-to-talk/ }));
    return screen.findByRole("button", { name: "Set key" });
  }

  it("captures a push-to-talk key with modifiers and saves the engine's value", async () => {
    const capture = vi.spyOn(api, "pttCaptureKey").mockResolvedValue("Ctrl+A");
    const update = vi.spyOn(api, "settingsUpdate");
    const button = await openPushToTalk();
    fireEvent.click(button);
    expect(button).toHaveAttribute("aria-pressed", "true");
    fireEvent.keyDown(button, { key: "Control", code: "ControlLeft", ctrlKey: true });
    expect(capture).not.toHaveBeenCalled();
    fireEvent.keyDown(button, { key: "a", code: "KeyA", ctrlKey: true });
    expect(capture).toHaveBeenCalledWith("Ctrl+KeyA");
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(
        expect.objectContaining({ talkMode: "pushToTalk", pttKey: "Ctrl+A" }),
      );
    });
    expect(await screen.findByText("Ctrl+A")).toBeInTheDocument();
  });

  it("cancels listening with Escape without leaving the settings", async () => {
    const onDocumentKey = vi.fn();
    document.addEventListener("keydown", onDocumentKey);
    const capture = vi.spyOn(api, "pttCaptureKey");
    const button = await openPushToTalk();
    fireEvent.click(button);
    fireEvent.keyDown(button, { key: "Escape", code: "Escape" });
    expect(capture).not.toHaveBeenCalled();
    expect(button).toHaveAttribute("aria-pressed", "false");
    expect(onDocumentKey).not.toHaveBeenCalled();
    document.removeEventListener("keydown", onDocumentKey);
  });

  it("cancels a pending desktop dialog", async () => {
    let reject: (error: unknown) => void = () => undefined;
    vi.spyOn(api, "pttCaptureKey").mockImplementation(
      () =>
        new Promise((_, fail) => {
          reject = fail;
        }),
    );
    const cancel = vi.spyOn(api, "pttCaptureCancel").mockImplementation(() => {
      reject({ code: "invalidInput", message: "cancelled" });
      return Promise.resolve(null);
    });
    const button = await openPushToTalk();
    fireEvent.click(button);
    fireEvent.keyDown(button, { key: "F9", code: "F9" });
    expect(await screen.findByText(/Confirm the shortcut in your desktop's dialog/)).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(cancel).toHaveBeenCalled();
    expect(await screen.findByRole("button", { name: "Set key" })).toBeInTheDocument();
    expect(screen.queryByText(/cannot be used/)).not.toBeInTheDocument();
  });

  it("says when the desktop did not confirm the shortcut", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    api.mock.failNext("pttCaptureKey", "shortcutNotConfirmed");
    const button = await openPushToTalk();
    fireEvent.click(button);
    fireEvent.keyDown(button, { key: "F8", code: "F8" });
    expect(await screen.findByText(/The desktop didn't confirm the shortcut/)).toBeVisible();
    expect(screen.queryByText(/cannot be used/)).not.toBeInTheDocument();
  });

  it("disables push-to-talk with the reason when it is unavailable", async () => {
    vi.spyOn(api, "pttAvailability").mockResolvedValue({
      status: "unavailable",
      reason: "waylandNoPortal",
    });
    vi.spyOn(api, "pttMouseStatus").mockResolvedValue({ support: "evdev", state: "ready" });
    await openSection("Voice & Audio");
    expect(await screen.findByText(/does not offer global shortcuts/)).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /Push-to-talk/ })).toBeDisabled();
  });

  it("captures a mouse side button and shows it by name", async () => {
    const capture = vi.spyOn(api, "pttCaptureKey");
    const update = vi.spyOn(api, "settingsUpdate");
    const button = await openPushToTalk();
    fireEvent.click(button);
    expect(button).toHaveAccessibleName(/mouse button/);
    // Left and right click are never captured.
    fireEvent.mouseDown(button, { button: 0 });
    fireEvent.mouseDown(button, { button: 2 });
    expect(capture).not.toHaveBeenCalled();
    fireEvent.mouseDown(button, { button: 3 });
    expect(capture).toHaveBeenCalledWith("Mouse4");
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ pttKey: "Mouse4" }));
    });
    expect(await screen.findByText("Mouse button 4 (back)")).toBeInTheDocument();
  });

  describe("on Wayland", () => {
    beforeEach(() => {
      vi.spyOn(api, "pttMouseStatus").mockResolvedValue({
        support: "evdev",
        state: "inputGroupMissing",
      });
    });

    it("keeps mouse buttons off until the user opts in, with a warning", async () => {
      const capture = vi.spyOn(api, "pttCaptureKey");
      const update = vi.spyOn(api, "settingsUpdate");
      const button = await openPushToTalk();
      const toggle = screen.getByRole("switch", { name: /Allow mouse buttons/ });
      expect(toggle).not.toBeChecked();
      expectTip(toggle, /read all input devices, including your keyboard/);
      expect(screen.queryByText("sudo usermod -aG input $USER")).not.toBeInTheDocument();
      fireEvent.click(button);
      fireEvent.mouseDown(button, { button: 4 });
      expect(capture).not.toHaveBeenCalled();
      fireEvent.click(toggle);
      await waitFor(() => {
        expect(update).toHaveBeenLastCalledWith(
          expect.objectContaining({ allowMouseButtons: true }),
        );
      });
    });

    it("shows the input group command when permission is missing", async () => {
      await api.settingsUpdate({ ...(await api.settingsGet()), allowMouseButtons: true });
      await openSection("Voice & Audio");
      expect(await screen.findByText("sudo usermod -aG input $USER")).toBeInTheDocument();
      expect(screen.getByText(/log out and back in/)).toBeInTheDocument();
    });

    it("offers push-to-talk without the portal once mouse buttons are allowed", async () => {
      vi.spyOn(api, "pttAvailability").mockResolvedValue({
        status: "unavailable",
        reason: "waylandNoPortal",
      });
      await api.settingsUpdate({ ...(await api.settingsGet()), allowMouseButtons: true });
      await openSection("Voice & Audio");
      await screen.findByText(/does not offer global shortcuts/);
      expect(screen.getByRole("radio", { name: /Push-to-talk/ })).toBeEnabled();
    });
  });

  describe("noise suppression (FR-3.65)", () => {
    it("offers Light, Standard, Advanced (AI) with a Beta badge and Off, Standard selected", async () => {
      await openSection("Voice & Audio");
      const group = await screen.findByRole("group", { name: "Noise suppression" });
      const radios = within(group).getAllByRole("radio");
      expect(radios.map((radio) => radio.closest("label")?.textContent)).toEqual([
        "Light",
        "Standard",
        "Advanced (AI) Beta",
        "Off",
      ]);
      expect(within(group).getByRole("radio", { name: "Standard Recommended" })).toBeChecked();
    });

    it("explains each level behind a ? and saves the choice", async () => {
      const update = vi.spyOn(api, "settingsUpdate");
      await openSection("Voice & Audio");
      const group = await screen.findByRole("group", { name: "Noise suppression" });
      expect(within(group).getByRole("button", { name: "About Advanced (AI)" })).toHaveAttribute(
        "data-tooltip",
        expect.stringMatching(/three times the CPU/),
      );
      expect(within(group).getByRole("button", { name: "About Light" })).toHaveAttribute(
        "data-tooltip",
        expect.stringMatching(/Lowest CPU/),
      );

      fireEvent.click(within(group).getByRole("radio", { name: /Advanced/ }));
      await waitFor(() => {
        expect(update).toHaveBeenLastCalledWith(
          expect.objectContaining({ noiseSuppression: true, noiseSuppressionBackend: "advanced" }),
        );
      });
      fireEvent.click(within(group).getByRole("radio", { name: "Light" }));
      await waitFor(() => {
        expect(update).toHaveBeenLastCalledWith(
          expect.objectContaining({ noiseSuppression: true, noiseSuppressionBackend: "light" }),
        );
      });
      fireEvent.click(within(group).getByRole("radio", { name: "Off" }));
      await waitFor(() => {
        expect(update).toHaveBeenLastCalledWith(
          expect.objectContaining({ noiseSuppression: false, noiseSuppressionBackend: "light" }),
        );
      });
    });
  });

  describe("compatibility mode (FR-1.48)", () => {
    it("is hidden outside Linux Wayland", async () => {
      await openSection("Voice & Audio");
      await screen.findByRole("group", { name: "Noise suppression" });
      expect(screen.queryByRole("switch", { name: /Compatibility mode/ })).not.toBeInTheDocument();
    });

    it("is off by default on Wayland and offers a restart when switched on", async () => {
      vi.spyOn(api, "pttSession").mockResolvedValue({
        waylandDesktop: true,
        compatibilityActive: false,
        kde: false,
      });
      vi.spyOn(api, "pttMouseStatus").mockResolvedValue({ support: "evdev", state: "ready" });
      const update = vi.spyOn(api, "settingsUpdate");
      const restart = vi.spyOn(api, "appRestart");
      await openSection("Voice & Audio");
      const toggle = await screen.findByRole("switch", { name: /Compatibility mode/ });
      expect(toggle).not.toBeChecked();
      expectTip(toggle, /Legacy X11 App Support/);
      expect(screen.queryByRole("button", { name: "Restart now" })).not.toBeInTheDocument();
      fireEvent.click(toggle);
      await waitFor(() => {
        expect(update).toHaveBeenLastCalledWith(
          expect.objectContaining({ pttCompatibilityMode: true }),
        );
      });
      fireEvent.click(await screen.findByRole("button", { name: "Restart now" }));
      expect(restart).toHaveBeenCalledOnce();
    });

    it("hides the input group option and explains the X11 path while active", async () => {
      vi.spyOn(api, "pttSession").mockResolvedValue({
        waylandDesktop: true,
        compatibilityActive: true,
        kde: false,
      });
      vi.spyOn(api, "pttMouseStatus").mockResolvedValue({ support: "evdev", state: "ready" });
      await api.settingsUpdate({ ...(await api.settingsGet()), pttCompatibilityMode: true });
      await openSection("Voice & Audio");
      expect(await screen.findByRole("switch", { name: /Compatibility mode/ })).toBeChecked();
      expect(screen.getByText(/Compatibility mode is on/)).toBeInTheDocument();
      expect(screen.queryByRole("switch", { name: /Allow mouse buttons/ })).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Restart now" })).not.toBeInTheDocument();
    });

    it("offers the KDE X11 settings only on KDE and reports a failure", async () => {
      vi.spyOn(api, "pttSession").mockResolvedValue({
        waylandDesktop: true,
        compatibilityActive: true,
        kde: true,
      });
      const open = vi.spyOn(api, "pttOpenKdeX11Settings");
      await openSection("Voice & Audio");
      const button = await screen.findByRole("button", { name: "Open KDE X11 settings" });
      fireEvent.click(button);
      expect(open).toHaveBeenCalledOnce();
      vi.spyOn(console, "error").mockImplementation(() => undefined);
      api.mock.failNext("pttOpenKdeX11Settings", "io");
      fireEvent.click(button);
      expect(await screen.findByText(/could not open the KDE settings/)).toBeInTheDocument();
    });

    it("asks before allowing mouse buttons on KDE, then offers a restart and undo", async () => {
      vi.spyOn(api, "pttSession").mockResolvedValue({
        waylandDesktop: true,
        compatibilityActive: true,
        kde: true,
      });
      const allow = vi.spyOn(api, "pttKdeX11AllowMouse");
      const undo = vi.spyOn(api, "pttKdeX11Undo");
      await openSection("Voice & Audio");
      fireEvent.click(await screen.findByRole("button", { name: "Allow mouse buttons" }));
      expect(allow).not.toHaveBeenCalled();
      fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
      expect(allow).not.toHaveBeenCalled();
      fireEvent.click(await screen.findByRole("button", { name: "Allow mouse buttons" }));
      fireEvent.click(await screen.findByRole("button", { name: "Allow" }));
      expect(await screen.findByText(/Restart Mumcord for it to take effect/)).toBeInTheDocument();
      expect(allow).toHaveBeenCalledOnce();
      expect(screen.queryByRole("button", { name: "Allow mouse buttons" })).toBeNull();
      fireEvent.click(screen.getByRole("button", { name: "Undo" }));
      await waitFor(() => {
        expect(undo).toHaveBeenCalledOnce();
      });
      expect(
        await screen.findByRole("button", { name: "Allow mouse buttons" }),
      ).toBeInTheDocument();
    });

    it("shows no allow button when KDE already allows mouse buttons", async () => {
      vi.spyOn(api, "pttSession").mockResolvedValue({
        waylandDesktop: true,
        compatibilityActive: true,
        kde: true,
      });
      vi.spyOn(api, "pttKdeX11Access").mockResolvedValue({ allowed: true, undoAvailable: false });
      await openSection("Voice & Audio");
      expect(await screen.findByText(/already allows mouse buttons/)).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Allow mouse buttons" })).toBeNull();
      expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
    });

    it("reports a failed change", async () => {
      vi.spyOn(api, "pttSession").mockResolvedValue({
        waylandDesktop: true,
        compatibilityActive: true,
        kde: true,
      });
      vi.spyOn(console, "error").mockImplementation(() => undefined);
      await openSection("Voice & Audio");
      fireEvent.click(await screen.findByRole("button", { name: "Allow mouse buttons" }));
      api.mock.failNext("pttKdeX11AllowMouse", "io");
      fireEvent.click(await screen.findByRole("button", { name: "Allow" }));
      expect(await screen.findByText(/could not change the KDE setting/)).toBeInTheDocument();
    });

    it("hides the KDE X11 settings on other desktops", async () => {
      vi.spyOn(api, "pttSession").mockResolvedValue({
        waylandDesktop: true,
        compatibilityActive: true,
        kde: false,
      });
      await openSection("Voice & Audio");
      await screen.findByRole("switch", { name: /Compatibility mode/ });
      expect(screen.queryByRole("button", { name: "Open KDE X11 settings" })).toBeNull();
    });

    it("reports a failed restart", async () => {
      vi.spyOn(api, "pttSession").mockResolvedValue({
        waylandDesktop: true,
        compatibilityActive: true,
        kde: false,
      });
      api.mock.failNext("appRestart", "internal");
      // Running in compatibility mode with the setting off: switching back is pending.
      await openSection("Voice & Audio");
      vi.spyOn(console, "error").mockImplementation(() => undefined);
      fireEvent.click(await screen.findByRole("button", { name: "Restart now" }));
      expect(await screen.findByText(/could not restart/)).toBeInTheDocument();
    });
  });

  it("explains the missing Accessibility permission on macOS", async () => {
    vi.spyOn(api, "pttMouseStatus").mockResolvedValue({
      support: "native",
      state: "accessibilityMissing",
    });
    await openSection("Voice & Audio");
    expect(await screen.findByText(/Privacy & Security → Accessibility/)).toBeInTheDocument();
  });

  it("offers a manual voice threshold as a slider over the live level meter", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Voice & Audio");
    const threshold = () => screen.queryByRole("slider", { name: "Voice activation threshold" });
    expect(threshold()).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: /Manual threshold/ }));
    const slider = await screen.findByRole("slider", { name: "Voice activation threshold" });
    expect(slider).toHaveValue("-40");
    expect(screen.getByText("-40 dBFS")).toBeInTheDocument();
    act(() => {
      api.mock.emit("micLevel", { level: 0.5 });
    });
    // The microphone test card on the same page shows the same level.
    for (const meter of screen.getAllByRole("meter", { name: "Microphone level" })) {
      expect(meter).toHaveAttribute("aria-valuenow", "50");
    }
    fireEvent.change(slider, { target: { value: "-30" } });
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(
        expect.objectContaining({ voiceActivation: "manual", voiceThresholdDbfs: -30 }),
      );
    });
    expect(await screen.findByText("-30 dBFS")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: /Automatic/ }));
    await waitFor(() => {
      expect(threshold()).not.toBeInTheDocument();
    });
  });

  it("hides the threshold in push-to-talk mode", async () => {
    await openSection("Voice & Audio");
    fireEvent.click(screen.getByRole("radio", { name: /Push-to-talk/ }));
    await screen.findByRole("button", { name: "Set key" });
    expect(screen.queryByRole("radio", { name: /Manual threshold/ })).not.toBeInTheDocument();
  });

  it("saves the voice processing switches", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Voice & Audio");
    fireEvent.click(screen.getByRole("switch", { name: "Echo cancellation" }));
    fireEvent.click(screen.getByRole("switch", { name: "Automatic volume" }));
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(
        expect.objectContaining({
          echoCancellation: false,
          autoGain: false,
        }),
      );
    });
  });
});

describe("Audio section music quality", () => {
  it("is off by default and saves the switch", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Voice & Audio");
    const toggle = screen.getByRole("switch", { name: "Music quality" });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ musicQuality: true }));
    });
  });
});

describe("Audio section video ducking (FR-2.76)", () => {
  it("is on at 30 % by default and saves the switch and the slider", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Voice & Audio");
    const toggle = screen.getByRole("switch", { name: "Lower video sound while people talk" });
    expect(toggle).toBeChecked();
    const slider = screen.getByRole("slider", { name: "Lowered volume" });
    expect(slider).toHaveValue("30");
    fireEvent.change(slider, { target: { value: "60" } });
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ videoDuckingPercent: 60 }));
    });
    fireEvent.click(toggle);
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ videoDucking: false }));
    });
    expect(slider).toBeDisabled();
  });
});

describe("Audio section bitrate slider", () => {
  it("defaults to 96 kbit/s, says the maximum depends on the server while not connected", async () => {
    await openSection("Voice & Audio");
    const slider = screen.getByRole("slider", { name: "Bitrate" });
    expect(slider).toHaveAttribute("aria-valuetext", "96 kbit/s");
    expect(slider).toHaveAttribute("min", "16");
    expect(screen.getByText(/Maximum: depends on server/)).toBeInTheDocument();
  });

  it("shows the server maximum, follows changes and saves a lower value live", async () => {
    vi.spyOn(api, "bitrateMax").mockResolvedValue(96);
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Voice & Audio");
    const slider = await screen.findByRole("slider", { name: "Bitrate" });
    await screen.findByText(/Maximum: 96 kbit\/s/);
    expect(slider).toHaveAttribute("max", "96");

    fireEvent.change(slider, { target: { value: "32" } });
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ bitrateKbps: 32 }));
    });
    expect(slider).toHaveAttribute("aria-valuetext", "32 kbit/s");

    act(() => {
      api.mock.emit("bitrateMaxChanged", { kbps: 384 });
    });
    await screen.findByText(/Maximum: 384 kbit\/s/);
    expect(slider).toHaveAttribute("max", "384");

    fireEvent.change(slider, { target: { value: "384" } });
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ bitrateKbps: null }));
    });
  });
});

describe("Chat & history section", () => {
  it("offers the preview cache age with 7 days as the default and saves a change", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Privacy & Connections");
    const select = screen.getByRole("combobox", { name: "Delete cached previews after" });
    expect(select).toHaveTextContent("7 days");
    expect(optionLabels(select)).toEqual(["1 day", "7 days", "30 days", "Never"]);
    chooseOption(select, "30 days");
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ previewCacheDays: 30 }));
    });
    chooseOption(select, "Never");
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ previewCacheDays: null }));
    });
  });

  it("clears the preview cache now and says how much space it freed", async () => {
    const clear = vi.spyOn(api, "previewCacheClear").mockResolvedValue(1_468_006);
    await openSection("Privacy & Connections");
    fireEvent.click(screen.getByRole("button", { name: "Clear now" }));
    expect(await screen.findByText("Cache cleared, 1.4 MB freed.")).toBeVisible();
    expect(clear).toHaveBeenCalledOnce();
  });

  it("shows the cache sizes per category and in total", async () => {
    vi.spyOn(api, "cacheSizes").mockResolvedValue({
      previewsKib: 1024,
      webKib: 2048,
      tempKib: 512,
    });
    await openSection("Advanced & Developer");
    expect(
      await screen.findByRole("switch", { name: "Link previews and images (1.0 MB)" }),
    ).toBeChecked();
    expect(screen.getByRole("switch", { name: "Web engine cache (2.0 MB)" })).toBeChecked();
    expect(screen.getByRole("switch", { name: "Temporary files (512.0 KB)" })).toBeChecked();
    expect(screen.getByText("Total: 3.5 MB")).toBeVisible();
  });

  it("clears only the selected caches after a confirmation that names what stays", async () => {
    const clear = vi
      .spyOn(api, "cacheClear")
      .mockResolvedValue({ freedKib: 1024, webPending: false });
    await openSection("Advanced & Developer");
    fireEvent.click(await screen.findByRole("switch", { name: /^Web engine cache/ }));
    fireEvent.click(screen.getByRole("button", { name: "Clear cache" }));
    expect(clear).not.toHaveBeenCalled();
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent(
      /chat history, settings, servers, certificates, friends and notes/,
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Clear cache" }));
    expect(await screen.findByText("Cache cleared (1.0 MB)")).toBeVisible();
    expect(clear).toHaveBeenCalledWith(["previews", "temp"]);
  });

  it("does not clear anything when the confirmation is cancelled or nothing is selected", async () => {
    const clear = vi.spyOn(api, "cacheClear");
    await openSection("Advanced & Developer");
    fireEvent.click(await screen.findByRole("button", { name: "Clear cache" }));
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: "Cancel" }),
    );
    expect(clear).not.toHaveBeenCalled();
    for (const name of [/^Link previews and images/, /^Web engine cache/, /^Temporary files/]) {
      fireEvent.click(screen.getByRole("switch", { name }));
    }
    expect(screen.getByRole("button", { name: "Clear cache" })).toBeDisabled();
  });

  it("says when the web engine cache goes at the next start", async () => {
    vi.spyOn(api, "cacheClear").mockResolvedValue({ freedKib: 2048, webPending: true });
    await openSection("Advanced & Developer");
    fireEvent.click(await screen.findByRole("button", { name: "Clear cache" }));
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: "Clear cache" }),
    );
    expect(await screen.findByText(/removed the next time Mumcord starts/)).toBeVisible();
  });

  it("reports a failed cache clean-up", async () => {
    vi.spyOn(api, "previewCacheClear").mockRejectedValue({ kind: "io", message: "x" });
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    await openSection("Privacy & Connections");
    fireEvent.click(screen.getByRole("button", { name: "Clear now" }));
    await waitFor(() => {
      expect(screen.getByRole("status")).not.toBeEmptyDOMElement();
    });
    expect(screen.queryByText(/freed/)).toBeNull();
  });

  it("deletes old history automatically only when asked: off by default, no confirmation", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Chat & Notifications");
    const select = screen.getByRole("combobox", { name: "Delete chat history older than" });
    expect(select).toHaveTextContent("Never (keep everything)");
    expect(optionLabels(select)).toEqual([
      "7 days",
      "30 days",
      "90 days",
      "365 days",
      "Never (keep everything)",
    ]);
    expectTip(select, /deleted from this computer automatically, without asking/);
    chooseOption(select, "90 days");
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ historyMaxAgeDays: 90 }));
    });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("switches history saving per server", async () => {
    const setSaving = vi.spyOn(api, "historySetSaving");
    await openSection("Servers");
    fireEvent.click(
      await screen.findByRole("switch", { name: "Save history on Local test server" }),
    );
    expect(setSaving).toHaveBeenCalledWith(1, false);
    await waitFor(() => {
      expect(
        screen.getByRole("switch", { name: "Save history on Local test server" }),
      ).not.toBeChecked();
    });
  });

  it("clears one server's history after confirmation", async () => {
    const clear = vi.spyOn(api, "historyClear");
    await openSection("Servers");
    const opener = await screen.findByRole("button", {
      name: "Clear history of Local test server…",
    });
    opener.focus();
    fireEvent.click(opener);
    const dialog = screen.getByRole("dialog", { name: "Clear history?" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(screen.getByRole("tablist", { hidden: true }).closest(".settings")).toHaveAttribute(
      "inert",
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Clear history" }));
    await waitFor(() => {
      expect(clear).toHaveBeenCalledWith({ kind: "server", serverId: 1 });
    });
    expect(await screen.findByText("History cleared.")).toBeInTheDocument();
    expect(opener).toHaveFocus();
  });

  it("clears all history after confirmation; Escape only closes the dialog", async () => {
    const onDocumentKey = vi.fn();
    document.addEventListener("keydown", onDocumentKey);
    const clear = vi.spyOn(api, "historyClear");
    await openSection("Chat & Notifications");
    fireEvent.click(screen.getByRole("button", { name: "Clear all history…" }));
    const dialog = screen.getByRole("dialog", { name: "Clear history?" });
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(onDocumentKey).not.toHaveBeenCalled();
    expect(clear).not.toHaveBeenCalled();
    document.removeEventListener("keydown", onDocumentKey);

    fireEvent.click(screen.getByRole("button", { name: "Clear all history…" }));
    fireEvent.click(screen.getByRole("button", { name: "Clear history" }));
    await waitFor(() => {
      expect(clear).toHaveBeenCalledWith({ kind: "all" });
    });
  });
});

describe("Identity section", () => {
  it("sets, previews and clears the default avatar and toggles automatic sending", async () => {
    openPath.mockResolvedValue("/home/me/me.png");
    const setDefault = vi.spyOn(api, "defaultAvatarSet");
    await openSection("Account & Identity");
    expect(await screen.findByText("No default avatar set.")).toBeInTheDocument();
    const clear = screen.getByRole("button", { name: "Clear" });
    expect(clear).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Choose…" }));
    expect(await screen.findByRole("img", { name: "Default avatar preview" })).toBeInTheDocument();
    expect(setDefault).toHaveBeenCalledWith("/home/me/me.png");

    fireEvent.click(clear);
    expect(await screen.findByText("No default avatar set.")).toBeInTheDocument();
    expect(setDefault).toHaveBeenLastCalledWith(null);

    const toggle = screen.getByRole("switch", { name: "Send to new servers automatically" });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    await waitFor(async () => {
      expect((await api.settingsGet()).sendDefaultAvatar).toBe(false);
    });
  });

  it("shows a plain error when the default avatar is not an image", async () => {
    openPath.mockResolvedValue("/home/me/notes.txt");
    api.mock.failNext("defaultAvatarSet", "invalidInput", "not an image");
    await openSection("Account & Identity");
    fireEvent.click(await screen.findByRole("button", { name: "Choose…" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "Default avatar preview" })).not.toBeInTheDocument();
  });

  it("lists the identities with their fingerprint and use", async () => {
    await openSection("Account & Identity");
    const list = await screen.findByRole("list", { name: "Identities" });
    expect(within(list).getByText("Default", { selector: "strong" })).toBeInTheDocument();
    expect(within(list).getByText(/Name: Zoe · Created/)).toBeInTheDocument();
    expect(within(list).getByText(/Fingerprint/)).toBeInTheDocument();
    expect(within(list).getByText(/Used by: Local test server/)).toBeInTheDocument();
    expect(within(list).getByRole("button", { name: "Export Default" })).toBeInTheDocument();
    expect(screen.queryByText(/^identity\./)).toBeNull();
  });

  it("creates an identity with a name and a label", async () => {
    const create = vi.spyOn(api, "identityCreate");
    await openSection("Account & Identity");
    fireEvent.click(await screen.findByRole("button", { name: "New identity" }));
    const dialog = await screen.findByRole("dialog", { name: "New identity" });
    fireEvent.change(within(dialog).getByLabelText("Name on servers"), {
      target: { value: "Zoe (Acme)" },
    });
    fireEvent.change(within(dialog).getByLabelText("Label"), { target: { value: "Work" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Create" }));
    await waitFor(() => {
      expect(create).toHaveBeenCalledWith("Zoe (Acme)", "Work");
    });
    expect(await screen.findByText("Work")).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("renames an identity without touching its certificate", async () => {
    const rename = vi.spyOn(api, "identityRename");
    await openSection("Account & Identity");
    fireEvent.click(await screen.findByRole("button", { name: "Rename Default" }));
    const dialog = await screen.findByRole("dialog", { name: "Rename Default" });
    fireEvent.change(within(dialog).getByLabelText("Label"), { target: { value: "Main" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(rename).toHaveBeenCalledWith(1, "Main");
    });
    expect(await screen.findByText("Main")).toBeInTheDocument();
  });

  it("shows the full fingerprint and the servers of an identity", async () => {
    await openSection("Account & Identity");
    fireEvent.click(await screen.findByRole("button", { name: "Show details of Default" }));
    const dialog = await screen.findByRole("dialog", { name: "Identity Default" });
    expect(within(dialog).getByText(/^[A-Z0-9]{2}(:[A-Z0-9]{2})+$/)).toBeInTheDocument();
    expect(within(dialog).getByText("Local test server")).toBeInTheDocument();
  });

  it("exports the identity with a password entered twice", async () => {
    savePath.mockResolvedValue("/home/me/id.p12");
    const exportIdentity = vi.spyOn(api, "identityExport");
    await openSection("Account & Identity");
    fireEvent.click(await screen.findByRole("button", { name: "Export Default" }));
    const dialog = await screen.findByRole("dialog", { name: "Export identity Default" });
    const submit = within(dialog).getByRole("button", { name: "Export" });

    fireEvent.click(submit);
    expect(within(dialog).getByRole("alert")).toHaveTextContent("Please enter a password.");
    fireEvent.change(within(dialog).getByLabelText("Password"), { target: { value: "secret" } });
    fireEvent.change(within(dialog).getByLabelText("Repeat password"), {
      target: { value: "other" },
    });
    fireEvent.click(submit);
    expect(within(dialog).getByRole("alert")).toHaveTextContent("The passwords do not match.");
    expect(exportIdentity).not.toHaveBeenCalled();

    fireEvent.change(within(dialog).getByLabelText("Repeat password"), {
      target: { value: "secret" },
    });
    fireEvent.click(submit);
    await waitFor(() => {
      expect(exportIdentity).toHaveBeenCalledWith(1, "/home/me/id.p12", "secret");
    });
    expect(await screen.findByText("Identity exported.")).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("does nothing when the save dialog is cancelled", async () => {
    savePath.mockResolvedValue(null);
    await openSection("Account & Identity");
    fireEvent.click(await screen.findByRole("button", { name: "Export Default" }));
    await waitFor(() => {
      expect(savePath).toHaveBeenCalled();
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("imports an identity as an additional entry", async () => {
    openPath.mockResolvedValue("/home/me/Old.p12");
    const importIdentity = vi.spyOn(api, "identityImport");
    await openSection("Account & Identity");
    fireEvent.click(await screen.findByRole("button", { name: "Import…" }));
    const dialog = await screen.findByRole("dialog", { name: "Import identity" });
    expect(within(dialog).getByLabelText("Label")).toHaveValue("Old");
    fireEvent.click(within(dialog).getByRole("button", { name: "Import…" }));
    await waitFor(() => {
      expect(importIdentity).toHaveBeenCalledWith("/home/me/Old.p12", null, "Old");
    });
    expect(await screen.findByText('Identity "Old" imported.')).toBeInTheDocument();
    const list = screen.getByRole("list", { name: "Identities" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(2);
  });

  it("keeps the import dialog open with an explanation when the password is wrong", async () => {
    openPath.mockResolvedValue("/home/me/Old.p12");
    api.mock.failNext("identityImport", "invalidInput");
    await openSection("Account & Identity");
    fireEvent.click(await screen.findByRole("button", { name: "Import…" }));
    const dialog = await screen.findByRole("dialog", { name: "Import identity" });
    fireEvent.change(within(dialog).getByLabelText(/File password/), {
      target: { value: "wrong" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Import…" }));
    expect(await within(dialog).findByText(/password is not valid/)).toBeInTheDocument();
  });

  it("deletes an identity only after the label was typed", async () => {
    const remove = vi.spyOn(api, "identityDelete");
    await api.identityCreate("Zoe (Acme)", "Work");
    await openSection("Account & Identity");
    fireEvent.click(await screen.findByRole("button", { name: "Delete Work" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete the identity Work?" });
    const confirm = within(dialog).getByRole("button", { name: "Delete" });
    expect(confirm).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("Type the identity label to confirm"), {
      target: { value: "Wor" },
    });
    expect(confirm).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("Type the identity label to confirm"), {
      target: { value: "Work" },
    });
    expect(confirm).toBeEnabled();
    fireEvent.click(confirm);
    await waitFor(() => {
      expect(remove).toHaveBeenCalledWith(2);
    });
    expect(await screen.findByText("Identity deleted.")).toBeInTheDocument();
  });

  it("never lets the last identity be deleted", async () => {
    await openSection("Account & Identity");
    fireEvent.click(await screen.findByRole("button", { name: "Delete Default" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete the identity Default?" });
    expect(within(dialog).getByText(/at least one identity/)).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Delete" })).toBeDisabled();
  });

  it("offers a backup before deleting", async () => {
    savePath.mockResolvedValue("/home/me/work.p12");
    await api.identityCreate("Zoe (Acme)", "Work");
    await openSection("Account & Identity");
    fireEvent.click(await screen.findByRole("button", { name: "Delete Work" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete the identity Work?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Back up first" }));
    expect(await screen.findByRole("dialog", { name: "Export identity Work" })).toBeInTheDocument();
  });

  it("makes another identity the default", async () => {
    const setDefault = vi.spyOn(api, "identitySetDefault");
    await api.identityCreate("Zoe (Acme)", "Work");
    await openSection("Account & Identity");
    fireEvent.click(
      await screen.findByRole("button", { name: "Set Work as the default identity" }),
    );
    await waitFor(() => {
      expect(setDefault).toHaveBeenCalledWith(2);
    });
    expect(await screen.findByText("Work is now the default identity.")).toBeInTheDocument();
  });
});

describe("Diagnostics section", () => {
  it("changes the log level and saves a diagnostic file", async () => {
    savePath.mockResolvedValue("/home/me/diag.txt");
    const update = vi.spyOn(api, "settingsUpdate");
    const save = vi.spyOn(api, "diagnosticsSave");
    await openSection("Advanced & Developer");
    chooseOption(
      screen.getByRole("combobox", { name: "Log detail" }),
      "Detailed (for troubleshooting)",
    );
    await waitFor(() => {
      expect(update).toHaveBeenCalledWith(expect.objectContaining({ logLevel: "debug" }));
    });
    fireEvent.click(screen.getByRole("button", { name: "Save diagnostic file…" }));
    await waitFor(() => {
      expect(save).toHaveBeenCalledWith("/home/me/diag.txt");
    });
    expect(await screen.findByText("Diagnostic file saved.")).toBeInTheDocument();
  });
});

describe("About section", () => {
  it("shows version, system, the AGPL license and opens the third-party licences", async () => {
    const open = vi.spyOn(api, "licencesOpen");
    await openSection("About");
    expect(await screen.findByText("0.0.0-mock")).toBeInTheDocument();
    expect(screen.getByText("mock")).toBeInTheDocument();
    const license = screen.getByLabelText("Mumcord license (AGPL-3.0)");
    expect(license.tagName).toBe("PRE");
    expect(license).toHaveAttribute("tabindex", "0");
    expect(license).toHaveTextContent("GNU AFFERO GENERAL PUBLIC LICENSE");
    await switchSection("Advanced & Developer");
    fireEvent.click(screen.getByRole("button", { name: "Show third-party licences" }));
    expect(open).toHaveBeenCalledOnce();
  });

  it("offers running the setup wizard again without the identity step", async () => {
    const opener = vi.fn();
    setSettingsOpener(opener);
    await openSection("About");
    await screen.findByText("0.0.0-mock");
    expect(screen.getByRole("button", { name: "More information: Setup" }).dataset.tooltip).toMatch(
      /Your current choices are filled in/,
    );
    fireEvent.click(screen.getByRole("button", { name: "Run setup wizard again" }));
    expect(view.openRerunWizard).toHaveBeenCalledOnce();
    expect(
      screen.getByText(/Your certificate, and with it your registered name/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Manage it in Identity" }));
    expect(opener).toHaveBeenCalledWith("account");
    setSettingsOpener(null);
  });

  it("opens What's new in its own window and the support links in the browser", async () => {
    const open = vi.spyOn(api, "changelogOpen");
    await openSection("About");
    await screen.findByText("0.0.0-mock");
    fireEvent.click(screen.getByRole("button", { name: "What's new" }));
    expect(open).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Report a bug" }));
    expect(openInBrowser).toHaveBeenLastCalledWith(
      "https://github.com/Mumcord/mumcord/issues/new?template=bug_report.yml",
    );
    fireEvent.click(screen.getByRole("button", { name: "Request a feature" }));
    expect(openInBrowser).toHaveBeenLastCalledWith(
      "https://github.com/Mumcord/mumcord/issues/new?template=feature_request.yml",
    );
  });

  it("shows a dev build's display version", async () => {
    const info = api.appInfo.bind(api);
    vi.spyOn(api, "appInfo").mockImplementation(async () => ({
      ...(await info()),
      version: "dev~a1b2c3d",
      packageVersion: "0.4.2",
    }));
    await openSection("About");
    expect(await screen.findByText("dev~a1b2c3d")).toBeInTheDocument();
  });

  it("turns off showing What's new after updates", async () => {
    await openSection("About");
    const auto = screen.getByRole("switch", {
      name: "Show “What's new” automatically after updates",
    });
    expect(auto).toBeChecked();
    fireEvent.click(auto);
    await waitFor(async () => {
      expect((await api.settingsGet()).whatsNewAuto).toBe(false);
    });
  });

  it("explains when the What's new window cannot be opened", async () => {
    vi.spyOn(api, "changelogOpen").mockRejectedValue(new Error("boom"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    await openSection("About");
    fireEvent.click(screen.getByRole("button", { name: "What's new" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The “What's new” window could not be opened.",
    );
  });

  it("explains when the licences window cannot be opened", async () => {
    vi.spyOn(api, "licencesOpen").mockRejectedValue(new Error("boom"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    await openSection("Advanced & Developer");
    fireEvent.click(screen.getByRole("button", { name: "Show third-party licences" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The third-party licences could not be opened.",
    );
  });
});

describe("Settings search", () => {
  const search = () => screen.getByRole("searchbox", { name: "Search settings" });

  it("lists matching settings by group with counts and highlights, and jumps to the control", async () => {
    renderScreen();
    fireEvent.change(search(), { target: { value: "noise" } });
    expect(screen.getByRole("tablist", { hidden: true })).not.toBeVisible();
    const results = screen.getByRole("group", { name: "Search results" });
    const voice = within(results).getByRole("region", { name: /^Voice & Audio/ });
    const count = within(voice).getAllByRole("button").length;
    expect(count).toBeGreaterThan(1);
    expect(within(voice).getByRole("heading")).toHaveTextContent(
      `Voice & Audio ${String(count)} results`,
    );
    const result = within(voice).getByRole("button", {
      name: /^Noise suppression,\s*Processing & quality$/,
    });
    expect(result.querySelector("mark")).toHaveTextContent("Noise");
    fireEvent.click(result);
    const choice = await screen.findByRole("radio", { name: "Standard Recommended" });
    await waitFor(() => {
      expect(choice).toHaveFocus();
    });
    expect(choice.closest("fieldset")).toHaveClass("settings-highlight");
    expect(search()).toHaveValue("");
    expect(screen.getByRole("tab", { name: "Voice & Audio" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("finds settings by their hint, ignoring accents", async () => {
    await applyLanguage("de");
    renderScreen();
    const field = screen.getByRole("searchbox", { name: "Einstellungen durchsuchen" });
    fireEvent.change(field, { target: { value: "sprunglinks" } });
    expect(screen.getByRole("button", { name: /^Sprunglinks/ })).toBeInTheDocument();
    fireEvent.change(field, { target: { value: "screenreader ansagen" } });
    expect(screen.getByRole("button", { name: /^Screenreader-Ansagen/ })).toBeInTheDocument();
  });

  it("opens a matching section and focuses its tab", async () => {
    renderScreen();
    fireEvent.change(search(), { target: { value: "notifications" } });
    fireEvent.click(screen.getByRole("button", { name: "Chat & Notifications" }));
    await waitFor(() => {
      expect(screen.getByRole("tab", { name: "Chat & Notifications" })).toHaveFocus();
    });
  });

  it("shows an empty state and clears with Escape without closing the settings", () => {
    const closed = vi.fn();
    document.addEventListener("keydown", closed);
    renderScreen();
    fireEvent.change(search(), { target: { value: "xyz" } });
    expect(screen.getByText("No settings match “xyz”.")).toBeInTheDocument();
    fireEvent.keyDown(search(), { key: "Escape" });
    expect(search()).toHaveValue("");
    expect(closed).not.toHaveBeenCalled();
    expect(screen.getByRole("tablist")).toBeVisible();
    document.removeEventListener("keydown", closed);
  });

  it("focuses the search field with Ctrl+F", () => {
    renderScreen();
    const outside = vi.fn((event: KeyboardEvent) => event.defaultPrevented);
    window.addEventListener("keydown", outside);
    fireEvent.keyDown(screen.getByRole("tab", { name: "General" }), { key: "f", ctrlKey: true });
    expect(search()).toHaveFocus();
    // The channel search behind the overlay sees the key as handled.
    expect(outside).toHaveReturnedWith(true);
    window.removeEventListener("keydown", outside);
  });

  it("keeps the section list in its own scrolling column", async () => {
    renderScreen();
    const sidebar = screen.getByRole("tablist").parentElement;
    expect(sidebar).toHaveClass("settings__sidebar");
    expect(sidebar).toContainElement(search());
    const { readFileSync } = await import("node:fs");
    const css = readFileSync("ui/src/features/settings/settings.css", "utf8");
    const rule = (selector: string) =>
      css.match(new RegExp(`(?:^|\\n)${selector.replace(".", "\\.")} \\{([^}]*)\\}`))?.[1] ?? "";
    expect(rule(".settings")).toMatch(/height:\s*100%/);
    expect(rule(".settings__sidebar")).toMatch(/overflow-y:\s*auto/);
    expect(rule(".settings__panel")).toMatch(/overflow-y:\s*auto/);
  });
});

describe("Accessibility section", () => {
  afterEach(() => {
    setHintPreference("keyboardHints", true);
    setHintPreference("announcements", true);
    setHintPreference("skipLinks", true);
  });

  it("offers the three hint switches, all on by default, and applies a change at once", async () => {
    await openSection("General");
    const names = ["Show keyboard hints", "Screen reader announcements", "Skip links"];
    for (const name of names) {
      expect(screen.getByRole("switch", { name })).toBeChecked();
    }
    fireEvent.click(screen.getByRole("switch", { name: "Skip links" }));
    // Off differs from the default, so the switch is now also named "Changed" (#161).
    expect(screen.getByRole("switch", { name: "Skip links Changed" })).not.toBeChecked();
    expect(hintPreferences().skipLinks).toBe(false);
  });
});

const OVERLAY_CARD = "In-game overlay";

describe("Overlay on macOS (QA.58)", () => {
  const onMacos = () =>
    vi.spyOn(api, "appInfo").mockResolvedValue({
      version: "0.0.0",
      packageVersion: "0.0.0",
      os: "macos aarch64",
      appimage: false,
      portable: false,
      distro: null,
    });

  it("hides the in-game overlay card and its search entries", async () => {
    onMacos();
    renderScreen();
    await switchSection("Gaming & Overlay");
    await screen.findAllByRole("heading", { name: /Positional audio/ });
    await waitFor(() => {
      expect(screen.queryByRole("region", { name: OVERLAY_CARD })).toBeNull();
    });
    fireEvent.change(screen.getByRole("searchbox", { name: "Search settings" }), {
      target: { value: OVERLAY_CARD },
    });
    expect(screen.queryByRole("group", { name: "Search results" })).toBeNull();
  });

  it("keeps the in-game overlay card and its search entries on Linux", async () => {
    vi.spyOn(api, "appInfo").mockResolvedValue({
      version: "0.0.0",
      packageVersion: "0.0.0",
      os: "linux x86_64",
      appimage: false,
      portable: false,
      distro: null,
    });
    renderScreen();
    await switchSection("Gaming & Overlay");
    expect(await screen.findByRole("region", { name: OVERLAY_CARD })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search settings" }), {
      target: { value: OVERLAY_CARD },
    });
    expect(screen.getByRole("group", { name: "Search results" })).toBeInTheDocument();
  });
});
