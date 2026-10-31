import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { setApi } from "../../api";
import { subscribeAnnouncements } from "../../a11y/announcer";
import { stubDialog } from "../channels/testUtils";
import { chooseOption, optionLabels } from "../../components/SelectTestUtils";
import { axeViolations } from "../../test/axe";
import { renderInWizard, setupWizardTest, type Harness } from "./testUtils";
import { Wizard } from "./Wizard";

let harness: Harness;

beforeEach(async () => {
  harness = await setupWizardTest();
});

afterEach(() => {
  setApi(undefined);
});

async function createIdentity(name = "Tobias") {
  fireEvent.change(await screen.findByLabelText("Name"), { target: { value: name } });
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  return screen.findByRole("heading", { name: "Microphone" });
}

describe("Wizard", () => {
  it("finishes with the defaults in four steps and saves wizardCompleted", async () => {
    renderInWizard(harness, <Wizard />);
    const micHeading = await createIdentity();
    await waitFor(() => expect(micHeading).toHaveFocus());
    expect(await harness.api.identityGet()).toMatchObject({ name: "Tobias", isDefault: true });

    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "Your privacy" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "New versions" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    const talkHeading = await screen.findByRole("heading", { name: "How do you want to talk?" });
    await waitFor(() => expect(talkHeading).toHaveFocus());
    expect(screen.getByRole("radio", { name: "Voice activation" })).toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: "Open mumble:// links with Mumcord" }),
    ).toBeChecked();

    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    const saved = await harness.api.settingsGet();
    expect(saved).toMatchObject({ wizardCompleted: true, talkMode: "voiceActivation" });
    expect(harness.applySettings).toHaveBeenCalledWith(saved);
    expect(await harness.api.linkHandlerStatus()).toBe(true);
    expect(saved.autostart).toBe(false);
    expect(saved.linkPreviews).toBe(true);
    expect(saved.historySharing).toBe(true);
  });

  it("announces the step through the focused heading's description", async () => {
    renderInWizard(harness, <Wizard />);
    const heading = await createIdentity();
    expect(heading).toHaveAccessibleDescription("Step 2 of 5");
  });

  it("politely announces each new step", async () => {
    const spoken: string[] = [];
    const unsubscribe = subscribeAnnouncements((a) => {
      if (a.politeness === "polite") spoken.push(a.message);
    });
    renderInWizard(harness, <Wizard />);
    await screen.findByRole("heading", { name: "Your identity" });
    expect(spoken).toEqual([]);
    await createIdentity();
    await waitFor(() => {
      expect(spoken).toEqual(["Microphone. Step 2 of 5"]);
    });
    unsubscribe();
  });

  it("completes the whole flow with the keyboard only (Enter submits each step)", async () => {
    vi.spyOn(harness.api, "pttCaptureKey").mockImplementation((combo) => Promise.resolve(combo));
    renderInWizard(harness, <Wizard />);
    const form = () => screen.getByRole("heading", { level: 2 }).closest("form") as HTMLFormElement;

    const name = await screen.findByLabelText("Name");
    name.focus();
    fireEvent.change(name, { target: { value: "Kim" } });
    fireEvent.submit(form());
    const micH = await screen.findByRole("heading", { name: "Microphone" });
    await waitFor(() => expect(micH).toHaveFocus());

    fireEvent.submit(form());
    await screen.findByRole("heading", { name: "Your privacy" });
    fireEvent.submit(form());
    await screen.findByRole("heading", { name: "New versions" });
    fireEvent.submit(form());
    const talkH = await screen.findByRole("heading", { name: "How do you want to talk?" });
    await waitFor(() => expect(talkH).toHaveFocus());

    // Arrow keys select radios natively; jsdom only simulates the resulting click.
    fireEvent.click(screen.getByRole("radio", { name: /Push to talk/ }));
    const choose = screen.getByRole("button", { name: "Choose key" });
    choose.focus();
    fireEvent.keyDown(choose, { key: "Enter", code: "Enter" });
    fireEvent.click(choose);
    fireEvent.keyDown(choose, { key: "F8", code: "F8", shiftKey: true });
    expect(await screen.findByText("Key: Shift+F8")).toBeInTheDocument();

    fireEvent.submit(form());
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    expect(await harness.api.settingsGet()).toMatchObject({
      talkMode: "pushToTalk",
      pttKey: "Shift+F8",
      wizardCompleted: true,
    });
    expect(await harness.api.identityGet()).toMatchObject({ name: "Kim" });
  });

  it("goes back with the keyboard-reachable Back button and keeps the identity", async () => {
    renderInWizard(harness, <Wizard />);
    await createIdentity("Anna");
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    const h = await screen.findByRole("heading", { name: "Your identity" });
    await waitFor(() => expect(h).toHaveFocus());
    expect(screen.getByText("Using identity Anna.")).toBeInTheDocument();
  });

  it("confirms an identity that already exists", async () => {
    await harness.api.identityCreate("Ben", null);
    renderInWizard(harness, <Wizard />);
    expect(await screen.findByText("Using identity Ben.")).toBeInTheDocument();
    expect(screen.queryByLabelText("Name")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByRole("heading", { name: "Microphone" })).toBeInTheDocument();
  });

  it("shows a retry when loading fails", async () => {
    harness.api.mock.failNext("settingsGet", "internal");
    renderInWizard(harness, <Wizard />);
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be loaded");
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("heading", { name: "Your identity" })).toBeInTheDocument();
  });

  it("still loads when push-to-talk and link handler status are unknown", async () => {
    harness.api.mock.failNext("pttAvailability", "notImplemented");
    harness.api.mock.failNext("linkHandlerStatus", "notImplemented");
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    renderInWizard(harness, <Wizard />);
    expect(await screen.findByRole("heading", { name: "Your identity" })).toBeInTheDocument();
  });

  it("keeps the wizard open when saving fails at the end", async () => {
    renderInWizard(harness, <Wizard />);
    await createIdentity();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "Your privacy" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "New versions" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "How do you want to talk?" });
    harness.api.mock.failNext("settingsUpdate", "internal");
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be saved");
    expect(harness.finishWizard).not.toHaveBeenCalled();
    expect((await harness.api.settingsGet()).wizardCompleted).toBe(false);
  });

  it("finishes even when registering the link handler fails, and says so", async () => {
    const spoken: string[] = [];
    const unsubscribe = subscribeAnnouncements((a) => spoken.push(a.message));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    renderInWizard(harness, <Wizard />);
    await createIdentity();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "Your privacy" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "New versions" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "How do you want to talk?" });
    harness.api.mock.failNext("linkHandlerRegister", "internal");
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    expect(spoken.join(" ")).toContain("mumble:// links");
    unsubscribe();
  });

  it("does not register the link handler when unticked", async () => {
    renderInWizard(harness, <Wizard />);
    await createIdentity();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "Your privacy" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "New versions" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    fireEvent.click(
      await screen.findByRole("checkbox", { name: "Open mumble:// links with Mumcord" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    expect(await harness.api.linkHandlerStatus()).toBe(false);
  });

  it("turns on start at login when ticked", async () => {
    renderInWizard(harness, <Wizard />);
    await createIdentity();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "Your privacy" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "New versions" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    fireEvent.click(await screen.findByRole("checkbox", { name: "Start Mumcord when I log in" }));
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    const saved = await harness.api.settingsGet();
    expect(saved).toMatchObject({ wizardCompleted: true, autostart: true, startMinimized: true });
    expect(harness.applySettings).toHaveBeenCalledWith(saved);
  });

  it("turns link previews off when unticked", async () => {
    renderInWizard(harness, <Wizard />);
    await createIdentity();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "Your privacy" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "New versions" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    const previews = await screen.findByRole("checkbox", { name: "Show link previews and embeds" });
    expect(previews).toBeChecked();
    expect(previews).toHaveAccessibleDescription(/see your IP address/);
    fireEvent.click(previews);
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    const saved = await harness.api.settingsGet();
    expect(saved).toMatchObject({ wizardCompleted: true, linkPreviews: false });
    expect(harness.applySettings).toHaveBeenCalledWith(saved);
  });

  it("offers channel history sharing, on by default, and turns it off when unticked", async () => {
    renderInWizard(harness, <Wizard />);
    await createIdentity();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "Your privacy" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "New versions" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    const sharing = await screen.findByRole("checkbox", { name: "Share my channel history" });
    expect(sharing).toBeChecked();
    expect(sharing).toHaveAccessibleDescription(/Private chats are never shared/);
    fireEvent.click(sharing);
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    expect(await harness.api.settingsGet()).toMatchObject({
      wizardCompleted: true,
      historySharing: false,
    });
  });

  it("finishes even when start at login cannot be set up, and says so", async () => {
    const spoken: string[] = [];
    const unsubscribe = subscribeAnnouncements((a) => spoken.push(a.message));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    const api = harness.api;
    const original = api.settingsUpdate.bind(api);
    vi.spyOn(api, "settingsUpdate").mockImplementation((next) =>
      next.autostart ? Promise.reject(new Error("refused")) : original(next),
    );
    renderInWizard(harness, <Wizard />);
    await createIdentity();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "Your privacy" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "New versions" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    fireEvent.click(await screen.findByRole("checkbox", { name: "Start Mumcord when I log in" }));
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    expect(await api.settingsGet()).toMatchObject({ wizardCompleted: true, autostart: false });
    expect(spoken.join(" ")).toContain("start when you log in");
    unsubscribe();
  });

  it.each(["dark", "light"] as const)("has no axe violations on any step (%s)", async (theme) => {
    harness = await setupWizardTest(theme);
    const { container } = renderInWizard(harness, <Wizard />);
    await screen.findByRole("heading", { name: "Your identity" });
    expect(await axeViolations(container)).toEqual([]);
    await createIdentity();
    expect(await axeViolations(container)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "Your privacy" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "New versions" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "How do you want to talk?" });
    fireEvent.click(screen.getByRole("radio", { name: /Push to talk/ }));
    expect(await axeViolations(container)).toEqual([]);
  });

  it("asks what to do about new versions, off by default, and saves the answer", async () => {
    renderInWizard(harness, <Wizard />);
    await createIdentity();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "Your privacy" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "New versions" });
    expect(screen.getByRole("radio", { name: "Off" })).toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: "Download automatically" }));
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "How do you want to talk?" });
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    const saved = await harness.api.settingsGet();
    expect(saved.updateCheck).toBe("yes");
    expect(saved.updateAutoDownload).toBe(true);
  });
});

describe("Wizard rerun from Settings (#149)", () => {
  let onClose: Mock<() => void>;
  let onManageIdentities: Mock<() => void>;

  beforeEach(async () => {
    stubDialog();
    harness = await setupWizardTest("dark", "ready");
    onClose = vi.fn<() => void>();
    onManageIdentities = vi.fn<() => void>();
  });

  function renderRerun() {
    return renderInWizard(
      harness,
      <Wizard mode="rerun" onClose={onClose} onManageIdentities={onManageIdentities} />,
    );
  }

  function next() {
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
  }

  async function toTalkStep() {
    next();
    await screen.findByRole("heading", { name: "Your privacy" });
    next();
    await screen.findByRole("heading", { name: "New versions" });
    next();
    return screen.findByRole("heading", { name: "How do you want to talk?" });
  }

  it("starts at the microphone, counts four steps and leaves out the identity", async () => {
    const identityCreate = vi.spyOn(harness.api, "identityCreate");
    const identityImport = vi.spyOn(harness.api, "identityImport");
    renderRerun();
    const heading = await screen.findByRole("heading", { name: "Microphone" });
    await waitFor(() => expect(heading).toHaveFocus());
    expect(heading).toHaveAccessibleDescription("Step 1 of 4");
    expect(screen.queryByRole("heading", { name: "Your identity" })).not.toBeInTheDocument();
    expect(identityCreate).not.toHaveBeenCalled();
    expect(identityImport).not.toHaveBeenCalled();
  });

  it("lists the steps with the identity marked as skipped", async () => {
    renderRerun();
    const steps = await screen.findByRole("list", { name: "Setup steps" });
    expect(within(steps).getAllByRole("listitem")).toHaveLength(5);
    expect(within(steps).getByText("Identity")).toBeInTheDocument();
    expect(within(steps).getAllByText(/skipped/)).toHaveLength(1);
    expect(within(steps).getByText("Microphone").closest("li")).toHaveAttribute(
      "aria-current",
      "step",
    );
  });

  it("says the identity is not changed and links to the identity manager", async () => {
    renderRerun();
    await screen.findByRole("heading", { name: "Microphone" });
    expect(screen.getByText(/Your identity is not changed/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Manage identities" }));
    expect(onManageIdentities).toHaveBeenCalledOnce();
  });

  it("prefills the saved values", async () => {
    const saved = await harness.api.settingsGet();
    await harness.api.settingsUpdate({
      ...saved,
      inputDevice: "mic-usb",
      talkMode: "pushToTalk",
      pttKey: "F8",
      updateCheck: "yes",
      linkPreviews: false,
      historySharing: false,
    });
    renderRerun();
    const mic = await screen.findByRole("combobox", { name: "Microphone" });
    await waitFor(() => {
      expect(optionLabels(mic)).toContain("USB Headset");
    });
    expect(mic).toHaveTextContent("USB Headset");
    next();
    await screen.findByRole("heading", { name: "Your privacy" });
    next();
    await screen.findByRole("heading", { name: "New versions" });
    expect(screen.getByRole("radio", { name: "Notify only" })).toBeChecked();
    next();
    await screen.findByRole("heading", { name: "How do you want to talk?" });
    expect(screen.getByRole("radio", { name: /Push to talk/ })).toBeChecked();
    expect(screen.getByText("Key: F8")).toBeInTheDocument();
    expect(
      screen.getByRole("checkbox", { name: "Show link previews and embeds" }),
    ).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Share my channel history" })).not.toBeChecked();
  });

  it("writes nothing before Finish and only the changed microphone at Finish", async () => {
    const settingsUpdate = vi.spyOn(harness.api, "settingsUpdate");
    renderRerun();
    const mic = await screen.findByRole("combobox", { name: "Microphone" });
    await waitFor(() => {
      expect(optionLabels(mic)).toContain("USB Headset");
    });
    chooseOption(mic, "USB Headset");
    await waitFor(() => {
      expect(mic).toHaveTextContent("USB Headset");
    });
    expect(settingsUpdate).not.toHaveBeenCalled();
    expect((await harness.api.settingsGet()).inputDevice).toBeNull();

    await toTalkStep();
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(onClose).toHaveBeenCalledOnce();
    });
    expect(harness.finishWizard).not.toHaveBeenCalled();
    const saved = await harness.api.settingsGet();
    expect(saved).toMatchObject({
      inputDevice: "mic-usb",
      talkMode: "voiceActivation",
      autostart: false,
      linkPreviews: true,
      historySharing: true,
      // Passing the step answers the question, so "unset" becomes a plain "no".
      updateCheck: "no",
      wizardCompleted: true,
    });
    expect(settingsUpdate).toHaveBeenCalledTimes(1);
    expect(harness.applySettings).toHaveBeenCalledWith(saved);
  });

  it("tests unsaved devices without saving them", async () => {
    const micTestStart = vi.spyOn(harness.api, "micTestStart");
    renderRerun();
    const mic = await screen.findByRole("combobox", { name: "Microphone" });
    await waitFor(() => {
      expect(optionLabels(mic)).toContain("USB Headset");
    });
    chooseOption(mic, "USB Headset");
    fireEvent.click(screen.getByRole("button", { name: "Test" }));
    await waitFor(() => {
      expect(micTestStart).toHaveBeenCalledWith({ inputDevice: "mic-usb", outputDevice: null });
    });
  });

  it("closes without asking when Cancel drops nothing", async () => {
    renderRerun();
    await screen.findByRole("heading", { name: "Microphone" });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledOnce();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("closes without asking when Cancel on the last step drops nothing", async () => {
    renderRerun();
    await screen.findByRole("heading", { name: "Microphone" });
    next();
    await screen.findByRole("heading", { name: "Your privacy" });
    next();
    await screen.findByRole("heading", { name: "New versions" });
    next();
    await screen.findByRole("heading", { name: "How do you want to talk?" });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledOnce();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("asks before Cancel drops a changed update check", async () => {
    renderRerun();
    await screen.findByRole("heading", { name: "Microphone" });
    next();
    await screen.findByRole("heading", { name: "Your privacy" });
    next();
    await screen.findByRole("heading", { name: "New versions" });
    expect(screen.getByRole("radio", { name: "Off" })).toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: "Notify only" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(await screen.findByRole("alertdialog")).toBeInTheDocument();
  });

  it("saves a changed update check on Finish", async () => {
    renderRerun();
    await screen.findByRole("heading", { name: "Microphone" });
    next();
    await screen.findByRole("heading", { name: "Your privacy" });
    next();
    await screen.findByRole("heading", { name: "New versions" });
    fireEvent.click(screen.getByRole("radio", { name: "Notify only" }));
    next();
    await screen.findByRole("heading", { name: "How do you want to talk?" });
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(onClose).toHaveBeenCalledOnce();
    });
    const saved = await harness.api.settingsGet();
    expect(saved.updateCheck).toBe("yes");
    expect(saved.updateAutoDownload).toBe(false);
  });

  it("asks before Cancel drops a changed microphone, and keeps everything on discard", async () => {
    renderRerun();
    const mic = await screen.findByRole("combobox", { name: "Microphone" });
    await waitFor(() => {
      expect(optionLabels(mic)).toContain("USB Headset");
    });
    chooseOption(mic, "USB Headset");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    const question = await screen.findByRole("alertdialog", { name: "Discard your changes?" });
    expect(within(question).getByRole("button", { name: "Keep editing" })).toHaveFocus();
    fireEvent.click(within(question).getByRole("button", { name: "Keep editing" }));
    expect(onClose).not.toHaveBeenCalled();
    expect(mic).toHaveTextContent("USB Headset");

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Discard changes",
      }),
    );
    expect(onClose).toHaveBeenCalledOnce();
    expect((await harness.api.settingsGet()).inputDevice).toBeNull();
  });

  it("writes only the changed start options at Finish and leaves the overlay alone", async () => {
    const saved = await harness.api.settingsGet();
    await harness.api.settingsUpdate({ ...saved, autostart: true });
    const overlaySetMode = vi.spyOn(harness.api, "ingameOverlaySetMode");
    const settingsUpdate = vi.spyOn(harness.api, "settingsUpdate");
    renderRerun();
    await screen.findByRole("heading", { name: "Microphone" });
    await toTalkStep();
    const autostart = screen.getByRole("checkbox", { name: "Start Mumcord when I log in" });
    expect(autostart).toBeChecked();
    fireEvent.click(autostart);
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(onClose).toHaveBeenCalledOnce();
    });
    expect(settingsUpdate).toHaveBeenCalledTimes(1);
    expect((await harness.api.settingsGet()).autostart).toBe(false);
    expect(overlaySetMode).not.toHaveBeenCalled();
  });

  it("keeps the wizard open when saving fails", async () => {
    renderRerun();
    const mic = await screen.findByRole("combobox", { name: "Microphone" });
    await waitFor(() => {
      expect(optionLabels(mic)).toContain("USB Headset");
    });
    chooseOption(mic, "USB Headset");
    await toTalkStep();
    harness.api.mock.failNext("settingsUpdate", "internal");
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be saved");
    expect(onClose).not.toHaveBeenCalled();
  });

  it.each(["dark", "light"] as const)("has no axe violations (%s)", async (theme) => {
    harness = await setupWizardTest(theme, "ready");
    const { container } = renderRerun();
    await screen.findByRole("heading", { name: "Microphone" });
    expect(await axeViolations(container)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(await axeViolations(container)).toEqual([]);
  });
});
