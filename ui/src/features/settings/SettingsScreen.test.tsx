import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import { PreferencesContext, type Preferences } from "../../app/preferences";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { applyThemeToDocument } from "../../theme/theme";
import { pickOpenPath, pickSavePath } from "./fileDialogs";
import { SettingsScreen } from "./SettingsScreen";

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
    expect(tabs).toHaveLength(10);
    const identity = screen.getByRole("tab", { name: "Identity" });
    expect(identity).toHaveAttribute("aria-selected", "true");
    expect(identity).toHaveAttribute("tabindex", "0");

    fireEvent.keyDown(identity, { key: "ArrowDown" });
    const audio = screen.getByRole("tab", { name: "Audio" });
    expect(audio).toHaveFocus();
    expect(audio).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel", { name: "Audio" })).toBeInTheDocument();

    fireEvent.keyDown(audio, { key: "End" });
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
    fireEvent.click(screen.getByRole("radio", { name: "Light" }));
    await waitFor(() => {
      expect(applySettings).toHaveBeenCalledWith(expect.objectContaining({ theme: "light" }));
    });
    expect(update).toHaveBeenCalledWith(expect.objectContaining({ theme: "light" }));

    fireEvent.click(screen.getByRole("radio", { name: "Minimize to the tray" }));
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(
        expect.objectContaining({ theme: "light", closeBehaviour: "minimizeToTray" }),
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
    await openSection("Sounds");
    const toggle = screen.getByRole("switch", { name: "You mute or unmute yourself" });
    fireEvent.click(toggle);
    await waitFor(() => {
      expect(update).toHaveBeenCalledWith(
        expect.objectContaining({
          sounds: { joinLeave: true, selfMute: false, selfDeaf: true, messages: true },
        }),
      );
    });
  });

  it("shows the overlay placeholder", async () => {
    await openSection("Overlay & positional audio");
    expect(screen.getByText(/coming in a later stage/)).toBeInTheDocument();
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
    expect(await screen.findByRole("option", { name: "Default ALSA Input" })).toBeInTheDocument();
    const mic = screen.getByRole("combobox", { name: "Microphone" });
    expect(mic).toHaveValue("");
    fireEvent.change(mic, { target: { value: "mic-usb" } });
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ inputDevice: "mic-usb" }));
    });
    fireEvent.change(mic, { target: { value: "" } });
    await waitFor(() => {
      expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ inputDevice: null }));
    });
  });

  it("follows hot-plugged devices", async () => {
    await openSection("Audio");
    await screen.findAllByRole("option", { name: "USB Headset" });
    act(() => {
      api.mock.emit("audioDevicesChanged", {
        devices: { inputs: [{ id: "new", name: "Webcam Mic", isDefault: false }], outputs: [] },
      });
    });
    expect(screen.getByRole("option", { name: "Webcam Mic" })).toBeInTheDocument();
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

  it("explains the missing Accessibility permission on macOS", async () => {
    vi.spyOn(api, "pttMouseStatus").mockResolvedValue({
      support: "native",
      state: "accessibilityMissing",
    });
    await openSection("Voice");
    expect(await screen.findByText(/Privacy & Security → Accessibility/)).toBeInTheDocument();
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

describe("Chat & history section", () => {
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
    fireEvent.change(screen.getByRole("combobox", { name: "Log detail" }), {
      target: { value: "debug" },
    });
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
  it("shows version, system, the MIT license and the third-party notices", async () => {
    await openSection("About");
    expect(await screen.findByText("0.0.0-mock")).toBeInTheDocument();
    expect(screen.getByText("mock")).toBeInTheDocument();
    const license = screen.getByLabelText("Mumcord license (MIT)");
    expect(license.tagName).toBe("PRE");
    expect(license).toHaveAttribute("tabindex", "0");
    expect(license).toHaveTextContent("MIT License");
    const notices = await screen.findByLabelText("Third-party notices");
    expect(notices).toHaveTextContent("Mumble");
    expect(notices).toHaveTextContent("BSD-3-Clause");
    expect(notices).toHaveAttribute("tabindex", "0");
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
