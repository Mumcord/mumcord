import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { hintPreferences, setHintPreference } from "../../a11y/hintPreferences";
import { createMockApi, setApi, type MockApi } from "../../api";
import { PreferencesContext, type Preferences } from "../../app/preferences";
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

async function setup(theme: "dark" | "light" = "dark") {
  await applyLanguage("en");
  applyThemeToDocument(theme);
  api = createMockApi();
  setApi(api);
  applySettings = vi.fn<Preferences["applySettings"]>();
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
    applySettings,
  };
  return render(
    <PreferencesContext.Provider value={preferences}>
      <main aria-label="Settings test">
        <h1>Settings</h1>
        <SettingsScreen />
      </main>
    </PreferencesContext.Provider>,
  );
}

async function openSection(name: string) {
  renderScreen();
  fireEvent.click(await screen.findByRole("tab", { name }));
  // Wait until the settings record has loaded.
  await waitFor(() => {
    expect(screen.queryByText("Loading settings…")).not.toBeInTheDocument();
  });
}

describe("SettingsScreen", () => {
  it("lists the sections as a vertical tab list operated with the arrow keys", () => {
    renderScreen();
    const list = screen.getByRole("tablist", { name: "Settings sections" });
    expect(list).toHaveAttribute("aria-orientation", "vertical");
    const tabs = within(list).getAllByRole("tab");
    expect(tabs).toHaveLength(13);
    const identity = screen.getByRole("tab", { name: "Identity" });
    expect(identity).toHaveAttribute("aria-selected", "true");
    expect(identity).toHaveAttribute("tabindex", "0");

    fireEvent.keyDown(identity, { key: "ArrowDown" });
    const servers = screen.getByRole("tab", { name: "Servers" });
    expect(servers).toHaveFocus();
    expect(servers).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel", { name: "Servers" })).toBeInTheDocument();

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
    for (const name of ["Identity", "Audio", "Voice", "Chat & history", "Appearance", "About"]) {
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
    await openSection("Appearance");
    const autostart = screen.getByRole("switch", { name: "Start Mumcord when I log in" });
    const minimized = screen.getByRole("switch", { name: "Start minimized" });
    expect(autostart).not.toBeChecked();
    expect(minimized).toBeChecked();
    expect(minimized).toBeDisabled();
    expect(minimized).toHaveAccessibleDescription(
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
    await openSection("Voice");
    const toggle = screen.getByRole("switch", { name: "Connect muted until I join a channel" });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ connectMuted: false }));
    });
  });

  it("switches link previews off and applies it to the chat right away", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Chat & history");
    const toggle = screen.getByRole("switch", { name: "Show link previews and embeds" });
    expect(toggle).toBeChecked();
    expect(toggle).toHaveAccessibleDescription(/see your IP address/);
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
    await openSection("Language");
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
    await openSection("Language");
    fireEvent.click(screen.getByRole("radio", { name: "Follow system" }));
    await waitFor(() => {
      expect(update).toHaveBeenCalledWith(expect.objectContaining({ language: null }));
    });
  });

  it("toggles notification sounds", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Sounds & notifications");
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
          },
        }),
      );
    });
  });

  it("has separate mention sound and notification switches, both on by default", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Sounds & notifications");
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
    await openSection("Sounds & notifications");
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

  it("chooses how message notifications are shown", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Sounds & notifications");
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
    await openSection("Overlay");
    expect(await screen.findByRole("heading", { name: "In-game overlay" })).toBeInTheDocument();
  });

  it("offers the in-game overlay through the engine", async () => {
    const setMode = vi.spyOn(api, "ingameOverlaySetMode");
    await openSection("Overlay");
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
    await openSection("Audio");
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
    await openSection("Audio");
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
    await openSection("Audio");
    vi.useFakeTimers();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Start test" }));
      await Promise.resolve();
    });
    expect(start).toHaveBeenCalled();
    expect(screen.getByText(/played back to you/)).toBeInTheDocument();
    act(() => {
      api.mock.emit("micLevel", { level: 0.5 });
    });
    expect(screen.getByRole("meter", { name: "Microphone level" })).toHaveAttribute(
      "aria-valuenow",
      "50",
    );
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
    await openSection("Audio");
    fireEvent.click(screen.getByRole("button", { name: "Start test" }));
    await screen.findByRole("button", { name: "Stop recording" });
    fireEvent.click(screen.getByRole("tab", { name: "Voice" }));
    expect(stop).toHaveBeenCalledTimes(1);
  });
});

describe("Voice section", () => {
  async function openPushToTalk() {
    await openSection("Voice");
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
    await openSection("Voice");
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
      expect(toggle).toHaveAccessibleDescription(/read all input devices, including your keyboard/);
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
      await openSection("Voice");
      expect(await screen.findByText("sudo usermod -aG input $USER")).toBeInTheDocument();
      expect(screen.getByText(/log out and back in/)).toBeInTheDocument();
    });

    it("offers push-to-talk without the portal once mouse buttons are allowed", async () => {
      vi.spyOn(api, "pttAvailability").mockResolvedValue({
        status: "unavailable",
        reason: "waylandNoPortal",
      });
      await api.settingsUpdate({ ...(await api.settingsGet()), allowMouseButtons: true });
      await openSection("Voice");
      await screen.findByText(/does not offer global shortcuts/);
      expect(screen.getByRole("radio", { name: /Push-to-talk/ })).toBeEnabled();
    });
  });

  describe("compatibility mode (FR-1.48)", () => {
    it("is hidden outside Linux Wayland", async () => {
      await openSection("Voice");
      await screen.findByRole("switch", { name: /Noise suppression/ });
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
      await openSection("Voice");
      const toggle = await screen.findByRole("switch", { name: /Compatibility mode/ });
      expect(toggle).not.toBeChecked();
      expect(toggle).toHaveAccessibleDescription(/Legacy X11 App Support/);
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
      await openSection("Voice");
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
      await openSection("Voice");
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
      await openSection("Voice");
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
      await openSection("Voice");
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
      await openSection("Voice");
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
      await openSection("Voice");
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
      await openSection("Voice");
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
    await openSection("Voice");
    expect(await screen.findByText(/Privacy & Security → Accessibility/)).toBeInTheDocument();
  });

  it("offers a manual voice threshold as a slider over the live level meter", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Voice");
    expect(screen.queryByRole("slider")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: /Manual threshold/ }));
    const slider = await screen.findByRole("slider", { name: "Voice activation threshold" });
    expect(slider).toHaveValue("-40");
    expect(screen.getByText("-40 dBFS")).toBeInTheDocument();
    act(() => {
      api.mock.emit("micLevel", { level: 0.5 });
    });
    expect(screen.getByRole("meter", { name: "Microphone level" })).toHaveAttribute(
      "aria-valuenow",
      "50",
    );
    fireEvent.change(slider, { target: { value: "-30" } });
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(
        expect.objectContaining({ voiceActivation: "manual", voiceThresholdDbfs: -30 }),
      );
    });
    expect(await screen.findByText("-30 dBFS")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: /Automatic/ }));
    await waitFor(() => {
      expect(screen.queryByRole("slider")).not.toBeInTheDocument();
    });
  });

  it("hides the threshold in push-to-talk mode", async () => {
    await openSection("Voice");
    fireEvent.click(screen.getByRole("radio", { name: /Push-to-talk/ }));
    await screen.findByRole("button", { name: "Set key" });
    expect(screen.queryByRole("radio", { name: /Manual threshold/ })).not.toBeInTheDocument();
  });

  it("saves the voice processing switches", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Voice");
    fireEvent.click(screen.getByRole("switch", { name: "Noise suppression" }));
    fireEvent.click(screen.getByRole("switch", { name: "Echo cancellation" }));
    fireEvent.click(screen.getByRole("switch", { name: "Automatic volume" }));
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(
        expect.objectContaining({
          noiseSuppression: false,
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
    await openSection("Audio");
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
    await openSection("Audio");
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
    await openSection("Audio");
    const slider = screen.getByRole("slider", { name: "Bitrate" });
    expect(slider).toHaveAttribute("aria-valuetext", "96 kbit/s");
    expect(slider).toHaveAttribute("min", "16");
    expect(screen.getByText(/Maximum: depends on server/)).toBeInTheDocument();
  });

  it("shows the server maximum, follows changes and saves a lower value live", async () => {
    vi.spyOn(api, "bitrateMax").mockResolvedValue(96);
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Audio");
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
    await openSection("Chat & history");
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
    await openSection("Chat & history");
    fireEvent.click(screen.getByRole("button", { name: "Clear now" }));
    expect(await screen.findByText("Cache cleared, 1.4 MB freed.")).toBeVisible();
    expect(clear).toHaveBeenCalledOnce();
  });

  it("reports a failed cache clean-up", async () => {
    vi.spyOn(api, "previewCacheClear").mockRejectedValue({ kind: "io", message: "x" });
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    await openSection("Chat & history");
    fireEvent.click(screen.getByRole("button", { name: "Clear now" }));
    await waitFor(() => {
      expect(screen.getByRole("status")).not.toBeEmptyDOMElement();
    });
    expect(screen.queryByText(/freed/)).toBeNull();
  });

  it("deletes old history automatically only when asked: off by default, no confirmation", async () => {
    const update = vi.spyOn(api, "settingsUpdate");
    await openSection("Chat & history");
    const select = screen.getByRole("combobox", { name: "Delete chat history older than" });
    expect(select).toHaveTextContent("Never (keep everything)");
    expect(optionLabels(select)).toEqual([
      "7 days",
      "30 days",
      "90 days",
      "365 days",
      "Never (keep everything)",
    ]);
    expect(
      screen.getByText(/deleted from this computer automatically, without asking/),
    ).toBeVisible();
    chooseOption(select, "90 days");
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ historyMaxAgeDays: 90 }));
    });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("switches history saving per server", async () => {
    const setSaving = vi.spyOn(api, "historySetSaving");
    await openSection("Chat & history");
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
    await openSection("Chat & history");
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
    await openSection("Chat & history");
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
    await openSection("Identity");
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
    await openSection("Identity");
    fireEvent.click(await screen.findByRole("button", { name: "Choose…" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "Default avatar preview" })).not.toBeInTheDocument();
  });

  it("exports the identity with a password entered twice", async () => {
    savePath.mockResolvedValue("/home/me/id.p12");
    const exportIdentity = vi.spyOn(api, "identityExport");
    await openSection("Identity");
    expect(await screen.findByText("Zoe")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Export identity…" }));
    const dialog = await screen.findByRole("dialog", { name: "Export identity" });
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
      expect(exportIdentity).toHaveBeenCalledWith("/home/me/id.p12", "secret");
    });
    expect(await screen.findByText("Identity exported.")).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("does nothing when the save dialog is cancelled", async () => {
    savePath.mockResolvedValue(null);
    await openSection("Identity");
    fireEvent.click(screen.getByRole("button", { name: "Export identity…" }));
    await waitFor(() => {
      expect(savePath).toHaveBeenCalled();
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("imports an identity after confirming the replacement", async () => {
    openPath.mockResolvedValue("/home/me/Old.p12");
    const importIdentity = vi.spyOn(api, "identityImport");
    await openSection("Identity");
    fireEvent.click(screen.getByRole("button", { name: "Import identity…" }));
    const dialog = await screen.findByRole("dialog", { name: "Import identity" });
    expect(dialog).toHaveTextContent("Importing replaces your current identity");
    fireEvent.click(within(dialog).getByRole("button", { name: "Replace and import" }));
    await waitFor(() => {
      expect(importIdentity).toHaveBeenCalledWith("/home/me/Old.p12", null);
    });
    expect(await screen.findByText('Identity "Old" imported.')).toBeInTheDocument();
  });

  it("keeps the import dialog open with an explanation when the password is wrong", async () => {
    openPath.mockResolvedValue("/home/me/Old.p12");
    api.mock.failNext("identityImport", "invalidInput");
    await openSection("Identity");
    fireEvent.click(screen.getByRole("button", { name: "Import identity…" }));
    const dialog = await screen.findByRole("dialog", { name: "Import identity" });
    fireEvent.change(within(dialog).getByLabelText(/File password/), {
      target: { value: "wrong" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Replace and import" }));
    expect(await within(dialog).findByText(/password is not valid/)).toBeInTheDocument();
  });
});

describe("Diagnostics section", () => {
  it("changes the log level and saves a diagnostic file", async () => {
    savePath.mockResolvedValue("/home/me/diag.txt");
    const update = vi.spyOn(api, "settingsUpdate");
    const save = vi.spyOn(api, "diagnosticsSave");
    await openSection("Diagnostics");
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
  it("shows version, system, the AGPL license and the third-party notices", async () => {
    await openSection("About");
    expect(await screen.findByText("0.0.0-mock")).toBeInTheDocument();
    expect(screen.getByText("mock")).toBeInTheDocument();
    const license = screen.getByLabelText("Mumcord license (AGPL-3.0)");
    expect(license.tagName).toBe("PRE");
    expect(license).toHaveAttribute("tabindex", "0");
    expect(license).toHaveTextContent("GNU AFFERO GENERAL PUBLIC LICENSE");
    const notices = await screen.findByLabelText("Third-party notices");
    expect(notices).toHaveTextContent("Mumble");
    expect(notices).toHaveTextContent("BSD-3-Clause");
    expect(notices).toHaveAttribute("tabindex", "0");
  });

  it("opens the changelog for the shown version and the support links in the browser", async () => {
    await openSection("About");
    await screen.findByText("0.0.0-mock");
    fireEvent.click(screen.getByRole("button", { name: "What's new" }));
    expect(openInBrowser).toHaveBeenLastCalledWith("https://mumcord.app/en/changelog#v0.0.0-mock");
    fireEvent.click(screen.getByRole("button", { name: "Report a bug" }));
    expect(openInBrowser).toHaveBeenLastCalledWith(
      "https://github.com/ToBee94/mumcord/issues/new?template=bug_report.yml",
    );
    fireEvent.click(screen.getByRole("button", { name: "Request a feature" }));
    expect(openInBrowser).toHaveBeenLastCalledWith(
      "https://github.com/ToBee94/mumcord/issues/new?template=feature_request.yml",
    );
  });

  it("links the German changelog for the German UI", async () => {
    await applyLanguage("de");
    await openSection("Über");
    await screen.findByText("0.0.0-mock");
    fireEvent.click(screen.getByRole("button", { name: "Neuigkeiten" }));
    expect(openInBrowser).toHaveBeenLastCalledWith("https://mumcord.app/changelog#v0.0.0-mock");
  });

  it("explains when the notices cannot be loaded", async () => {
    vi.spyOn(api, "appInfo").mockRejectedValue(new Error("boom"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    await openSection("About");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The third-party notices could not be loaded.",
    );
  });
});

describe("Settings search", () => {
  const search = () => screen.getByRole("searchbox", { name: "Search settings" });

  it("lists matching settings as Section › Setting and jumps to the control", async () => {
    renderScreen();
    fireEvent.change(search(), { target: { value: "noise" } });
    expect(screen.getByRole("tablist", { hidden: true })).not.toBeVisible();
    const results = screen.getByRole("list", { name: "Search results" });
    fireEvent.click(within(results).getByRole("button", { name: "Voice › Noise suppression" }));
    const toggle = await screen.findByRole("switch", { name: "Noise suppression" });
    await waitFor(() => {
      expect(toggle).toHaveFocus();
    });
    expect(toggle.closest(".settings-switch")).toHaveClass("settings-highlight");
    expect(search()).toHaveValue("");
    expect(screen.getByRole("tab", { name: "Voice" })).toHaveAttribute("aria-selected", "true");
  });

  it("finds settings by their hint, ignoring accents", async () => {
    await applyLanguage("de");
    renderScreen();
    const field = screen.getByRole("searchbox", { name: "Einstellungen durchsuchen" });
    fireEvent.change(field, { target: { value: "sprunglinks" } });
    expect(
      screen.getByRole("button", { name: "Barrierefreiheit › Sprunglinks" }),
    ).toBeInTheDocument();
    fireEvent.change(field, { target: { value: "screenreader ansagen" } });
    expect(
      screen.getByRole("button", { name: "Barrierefreiheit › Screenreader-Ansagen" }),
    ).toBeInTheDocument();
  });

  it("opens a matching section and focuses its tab", async () => {
    renderScreen();
    fireEvent.change(search(), { target: { value: "sounds" } });
    fireEvent.click(screen.getByRole("button", { name: "Sounds & notifications" }));
    await waitFor(() => {
      expect(screen.getByRole("tab", { name: "Sounds & notifications" })).toHaveFocus();
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
    fireEvent.keyDown(screen.getByRole("tab", { name: "Identity" }), { key: "f", ctrlKey: true });
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
    await openSection("Accessibility");
    const names = ["Show keyboard hints", "Screen reader announcements", "Skip links"];
    for (const name of names) {
      expect(screen.getByRole("switch", { name })).toBeChecked();
    }
    fireEvent.click(screen.getByRole("switch", { name: "Skip links" }));
    expect(screen.getByRole("switch", { name: "Skip links" })).not.toBeChecked();
    expect(hintPreferences().skipLinks).toBe(false);
  });
});

describe("Overlay on macOS (QA.58)", () => {
  const onMacos = () =>
    vi.spyOn(api, "appInfo").mockResolvedValue({
      version: "0.0.0",
      os: "macos aarch64",
      thirdPartyNotices: "",
      appimage: false,
      distro: null,
    });

  it("hides the Overlay section and its search entries", async () => {
    onMacos();
    renderScreen();
    await waitFor(() => {
      expect(screen.queryByRole("tab", { name: "Overlay" })).toBeNull();
    });
    expect(screen.getAllByRole("tab")).toHaveLength(12);
    fireEvent.change(screen.getByRole("searchbox", { name: "Search settings" }), {
      target: { value: "overlay" },
    });
    expect(screen.queryByRole("list", { name: "Search results" })).toBeNull();
  });

  it("keeps the Overlay section and its search entries on Linux", async () => {
    vi.spyOn(api, "appInfo").mockResolvedValue({
      version: "0.0.0",
      os: "linux x86_64",
      thirdPartyNotices: "",
      appimage: false,
      distro: null,
    });
    renderScreen();
    expect(await screen.findByRole("tab", { name: "Overlay" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search settings" }), {
      target: { value: "overlay" },
    });
    expect(screen.getByRole("list", { name: "Search results" })).toBeInTheDocument();
  });
});
