import { fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { setApi, type PttAvailability, type PttMouseStatus, type Settings } from "../../api";
import { TalkModeStep, type TalkChoice } from "./TalkModeStep";
import { renderInWizard, setupWizardTest, type Harness } from "./testUtils";

let harness: Harness;
let settings: Settings;
let onFinish: Mock<(choice: TalkChoice) => Promise<void>>;

beforeEach(async () => {
  harness = await setupWizardTest();
  settings = await harness.api.settingsGet();
  onFinish = vi.fn<(choice: TalkChoice) => Promise<void>>(() => Promise.resolve());
});

afterEach(() => {
  setApi(undefined);
});

function renderStep(
  availability: PttAvailability | null = { status: "available" },
  linkHandlerActive: boolean | null = false,
  mouseStatus: PttMouseStatus | null = null,
) {
  renderInWizard(
    harness,
    <TalkModeStep
      settings={settings}
      availability={availability}
      mouseStatus={mouseStatus}
      linkHandlerActive={linkHandlerActive}
      focusOnMount={false}
      onBack={vi.fn()}
      onFinish={onFinish}
    />,
  );
}

function startCapture() {
  fireEvent.click(screen.getByRole("radio", { name: /Push to talk/ }));
  const button = screen.getByRole("button", { name: "Choose key" });
  button.focus();
  fireEvent.click(button);
  return screen.getByRole("button", { name: "Press a key… (Escape to cancel)" });
}

describe("TalkModeStep", () => {
  it("preselects voice activation and finishes with it", async () => {
    renderStep();
    expect(screen.getByRole("radio", { name: /Voice activation/ })).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(onFinish).toHaveBeenCalledWith({
        talkMode: "voiceActivation",
        pttKey: null,
        registerLinkHandler: true,
        autostart: false,
      });
    });
  });

  it("offers start at login, off by default, and passes the choice on", async () => {
    renderStep();
    const autostart = screen.getByRole("checkbox", { name: "Start Mumcord when I log in" });
    expect(autostart).not.toBeChecked();
    expect(autostart).toHaveAccessibleDescription(
      "You can change this later in Settings → Appearance.",
    );
    fireEvent.click(autostart);
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(onFinish).toHaveBeenCalledWith(expect.objectContaining({ autostart: true }));
    });
  });

  it("captures a key with modifiers and stores what the engine returns", async () => {
    const capture = vi.spyOn(harness.api, "pttCaptureKey").mockResolvedValue("Ctrl+A");
    renderStep();
    const button = startCapture();
    expect(button).toHaveAttribute("aria-pressed", "true");
    // A modifier on its own keeps waiting for the real key.
    fireEvent.keyDown(button, { key: "Control", code: "ControlLeft", ctrlKey: true });
    expect(capture).not.toHaveBeenCalled();
    fireEvent.keyDown(button, { key: "a", code: "KeyA", ctrlKey: true });
    expect(await screen.findByText("Key: Ctrl+A")).toBeInTheDocument();
    expect(capture).toHaveBeenCalledWith("Ctrl+KeyA");
    expect(screen.getByRole("button", { name: "Change key" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );

    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(onFinish).toHaveBeenCalledWith({
        talkMode: "pushToTalk",
        pttKey: "Ctrl+A",
        registerLinkHandler: true,
        autostart: false,
      });
    });
  });

  it("explains a key the engine rejects", async () => {
    harness.api.mock.failNext("pttCaptureKey", "invalidInput");
    renderStep();
    fireEvent.keyDown(startCapture(), { key: "CapsLock", code: "CapsLock" });
    expect(await screen.findByRole("alert")).toHaveTextContent("can't be used");
    expect(screen.getByRole("button", { name: "Choose key" })).toHaveAccessibleDescription(
      /can't be used/,
    );
  });

  it("says when the desktop did not confirm the shortcut", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    harness.api.mock.failNext("pttCaptureKey", "shortcutNotConfirmed");
    renderStep();
    fireEvent.keyDown(startCapture(), { key: "F8", code: "F8" });
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("The desktop didn't confirm the shortcut");
    expect(alert).not.toHaveTextContent("can't be used");
  });

  it("cancels listening with Escape without calling the engine", () => {
    const capture = vi.spyOn(harness.api, "pttCaptureKey");
    renderStep();
    fireEvent.keyDown(startCapture(), { key: "Escape", code: "Escape" });
    expect(screen.getByRole("button", { name: "Choose key" })).toBeInTheDocument();
    expect(capture).not.toHaveBeenCalled();
  });

  it("cancels a pending desktop dialog with Escape and shows no error", async () => {
    let reject: (error: unknown) => void = () => undefined;
    vi.spyOn(harness.api, "pttCaptureKey").mockImplementation(
      () =>
        new Promise((_resolve, rejectPromise) => {
          reject = rejectPromise;
        }),
    );
    const cancel = vi.spyOn(harness.api, "pttCaptureCancel");
    renderStep();
    fireEvent.keyDown(startCapture(), { key: "F13", code: "F13" });
    const pending = await screen.findByRole("button", {
      name: /Confirm the shortcut in the system dialog/,
    });
    fireEvent.keyDown(pending, { key: "Escape", code: "Escape" });
    expect(cancel).toHaveBeenCalled();
    reject(new Error("cancelled"));
    expect(await screen.findByRole("button", { name: "Choose key" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("stops listening when focus leaves with Tab", () => {
    renderStep();
    const button = startCapture();
    fireEvent.keyDown(button, { key: "Tab", code: "Tab" });
    fireEvent.blur(button);
    expect(screen.getByRole("button", { name: "Choose key" })).toBeInTheDocument();
  });

  it("requires a key before finishing with push to talk", async () => {
    renderStep();
    fireEvent.click(screen.getByRole("radio", { name: /Push to talk/ }));
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Choose a push-to-talk key");
    expect(screen.getByRole("button", { name: "Choose key" })).toHaveFocus();
    expect(onFinish).not.toHaveBeenCalled();
  });

  it.each([
    ["waylandNoPortal", "doesn't let apps use global shortcuts"],
    ["registrationFailed", "couldn't be set up"],
  ] as const)("shows push to talk as unavailable (%s)", (reason, text) => {
    renderStep({ status: "unavailable", reason });
    const radio = screen.getByRole("radio", { name: /Push to talk/ });
    expect(radio).toBeDisabled();
    expect(radio).toHaveAccessibleDescription(new RegExp(text));
    expect(
      screen.getByText(/Voice activation works without a key|Use voice activation/),
    ).toBeVisible();
    expect(screen.getByRole("radio", { name: /Voice activation/ })).toBeChecked();
  });

  it("skips the link offer when Mumcord already handles mumble:// links", async () => {
    renderStep({ status: "available" }, true);
    expect(
      screen.queryByRole("checkbox", { name: "Open mumble:// links with Mumcord" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("Mumcord already opens mumble:// links.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(onFinish).toHaveBeenCalledWith(
        expect.objectContaining({ registerLinkHandler: false }),
      );
    });
  });

  it("shows a save failure and allows another try", async () => {
    onFinish.mockRejectedValueOnce(new Error("nope"));
    renderStep();
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be saved");
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(onFinish).toHaveBeenCalledTimes(2);
    });
  });

  it("captures a mouse side button where mouse buttons work natively", async () => {
    const capture = vi.spyOn(harness.api, "pttCaptureKey");
    renderStep({ status: "available" }, false, { support: "native", state: "ready" });
    fireEvent.click(screen.getByRole("radio", { name: /Push to talk/ }));
    const button = screen.getByRole("button", { name: "Choose key" });
    fireEvent.click(button);
    fireEvent.mouseDown(button, { button: 0 });
    expect(capture).not.toHaveBeenCalled();
    fireEvent.mouseDown(button, { button: 4 });
    expect(capture).toHaveBeenCalledWith("Mouse5");
    expect(await screen.findByText("Key: Mouse button 5 (forward)")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(onFinish).toHaveBeenCalledWith(expect.objectContaining({ pttKey: "Mouse5" }));
    });
  });

  it("points to the opt-in on Wayland and ignores mouse buttons until then", () => {
    const capture = vi.spyOn(harness.api, "pttCaptureKey");
    renderStep({ status: "available" }, false, { support: "evdev", state: "ready" });
    const button = startCapture();
    expect(screen.getByText(/Allow mouse buttons/)).toBeInTheDocument();
    fireEvent.mouseDown(button, { button: 3 });
    expect(capture).not.toHaveBeenCalled();
  });
});
