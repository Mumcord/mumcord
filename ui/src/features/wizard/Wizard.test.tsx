import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { setApi, type Settings } from "../../api";
import { subscribeAnnouncements } from "../../a11y/announcer";
import { stubDialog } from "../channels/testUtils";
import { chooseOption, optionLabels } from "../../components/SelectTestUtils";
import { axeViolations } from "../../test/axe";
import { PRESETS } from "./presets";
import { renderInWizard, setupWizardTest, type Harness } from "./testUtils";
import { Wizard } from "./Wizard";

let harness: Harness;

beforeEach(async () => {
  harness = await setupWizardTest();
});

afterEach(() => {
  setApi(undefined);
});

const heading = (name: string | RegExp) => screen.findByRole("heading", { name });

function click(name: string | RegExp) {
  fireEvent.click(screen.getByRole("button", { name }));
}

/** Waits for the welcome step and presses its button. */
async function start() {
  fireEvent.click(await screen.findByRole("button", { name: "Let's get started" }));
}

/** Welcome → Identity (creates one) → Voice; resolves with the voice heading. */
async function toVoice(name = "Tobias") {
  await start();
  fireEvent.change(await screen.findByLabelText("Default display name"), {
    target: { value: name },
  });
  click("Next");
  return heading("Sound good?");
}

async function toSetupStyle() {
  await toVoice();
  click("Next");
  return heading("How much do you want to share?");
}

/** The values a preset covers, read from saved settings, keyed like `PRESETS`. */
function presetValues(settings: Settings) {
  return {
    "privateMessages.rule": settings.privateMessages.rule,
    "privateMessages.allowCurrentChannel": settings.privateMessages.allowCurrentChannel,
    "privateMessages.allowAccepted": settings.privateMessages.allowAccepted,
    "privateMessages.keepRequests": settings.privateMessages.keepRequests,
    p2pMode: settings.p2pMode,
    linkPreviews: settings.linkPreviews,
    offerFullImages: settings.offerFullImages,
    loadFullImages: settings.loadFullImages,
    historySharing: settings.historySharing,
    nowPlaying: settings.nowPlaying,
    nowGaming: settings.nowGaming,
  };
}

/** A preset's values without the two that live behind their own commands. */
function withoutCommands(values: (typeof PRESETS)["balanced"]) {
  return Object.fromEntries(
    Object.entries(values).filter(([key]) => key !== "ingameOverlay" && key !== "dndFollowSystem"),
  );
}

describe("Wizard (first start)", () => {
  it("walks Welcome, Identity, Voice and Setup style, and Balanced saves its values at the end", async () => {
    renderInWizard(harness, <Wizard />);
    await heading("Voice chat that feels like your own place.");
    const identity = await (async () => {
      click("Let's get started");
      return heading("Who are you on Mumble?");
    })();
    await waitFor(() => expect(identity).toHaveFocus());
    expect(identity).toHaveAccessibleDescription("Step 2 of 10 · Identity");
    fireEvent.change(screen.getByLabelText("Default display name"), {
      target: { value: "Tobias" },
    });
    click("Next");
    await heading("Sound good?");
    click("Next");
    await heading("How much do you want to share?");
    expect(screen.getByRole("radio", { name: "Balanced Recommended" })).toBeChecked();
    click("Finish setup");
    await heading("You're all set, Tobias");
    // Nothing is written before the last button.
    expect(await harness.api.settingsGet()).toMatchObject({
      wizardCompleted: false,
      setupPreset: null,
    });

    click("Start Mumcord");
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    const saved = await harness.api.settingsGet();
    expect(saved).toMatchObject({ wizardCompleted: true, setupPreset: "balanced" });
    expect(presetValues(saved)).toEqual(withoutCommands(PRESETS.balanced));
    expect((await harness.api.ingameOverlayState()).mode).toBe("on");
    expect((await harness.api.dndState()).followSystem).toBe(true);
    expect(await harness.api.linkHandlerStatus()).toBe(true);
    expect(harness.applySettings).toHaveBeenLastCalledWith(saved);
  });

  it("shows the step rail with the current step, checks, Custom tags and progress", async () => {
    renderInWizard(harness, <Wizard />);
    const rail = await screen.findByRole("navigation", { name: "Setup steps" });
    const items = within(rail).getAllByRole("listitem");
    expect(items.map((item) => item.getAttribute("aria-current"))).toEqual([
      "step",
      ...Array<null>(9).fill(null),
    ]);
    expect(within(rail).getAllByText("Custom")).toHaveLength(5);
    expect(within(rail).getByRole("progressbar")).toHaveAttribute("aria-valuenow", "0");
    expect(screen.getByText("Everything here can be changed later in Settings.")).toBeVisible();

    await toVoice();
    const after = within(rail).getAllByRole("listitem");
    expect(after[0]).toHaveTextContent("Welcome(done)");
    expect(after[2]).toHaveAttribute("aria-current", "step");
  });

  it("politely announces each new step", async () => {
    const spoken: string[] = [];
    const unsubscribe = subscribeAnnouncements((a) => {
      if (a.politeness === "polite") spoken.push(a.message);
    });
    renderInWizard(harness, <Wizard />);
    await heading("Voice chat that feels like your own place.");
    expect(spoken).toEqual([]);
    click("Let's get started");
    await waitFor(() => {
      expect(spoken).toEqual(["Who are you on Mumble?. Step 2 of 10"]);
    });
    unsubscribe();
  });

  it("previews the language and look at once but saves them only at Finish", async () => {
    renderInWizard(harness, <Wizard />);
    await heading("Voice chat that feels like your own place.");
    chooseOption(screen.getByRole("combobox", { name: "Display language" }), "Deutsch");
    await waitFor(() => {
      expect(harness.applySettings).toHaveBeenLastCalledWith(
        expect.objectContaining({ language: "de" }),
      );
    });
    expect((await harness.api.settingsGet()).language).toBeNull();
  });

  it("walks the five Custom steps and records custom when the values match no preset", async () => {
    renderInWizard(harness, <Wizard />);
    await toSetupStyle();
    fireEvent.click(screen.getByRole("radio", { name: "Custom" }));
    click("Next");
    await heading("Who can message you?");
    fireEvent.click(screen.getByRole("radio", { name: "Everyone" }));
    click("Next");
    await heading("Connections and content");
    fireEvent.click(screen.getByRole("radio", { name: "Relay only" }));
    click("Next");
    await heading("Make it yours");
    click("Skip for now");
    await heading("Pick your look");
    click("Skip for now");
    await heading("Startup and updates");
    fireEvent.click(screen.getByRole("switch", { name: "Start Mumcord when I log in" }));
    click("Next");
    await heading("You're all set, Tobias");
    click("Start Mumcord");
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    expect(await harness.api.settingsGet()).toMatchObject({
      setupPreset: "custom",
      p2pMode: "relayOnly",
      autostart: true,
    });
    expect((await harness.api.settingsGet()).privateMessages.rule).toBe("everyone");
  });

  it("records the preset that Custom values match exactly", async () => {
    renderInWizard(harness, <Wizard />);
    await toSetupStyle();
    fireEvent.click(screen.getByRole("radio", { name: "Custom" }));
    for (const step of ["Who can message you?", "Connections and content", "Make it yours"]) {
      click("Next");
      await heading(step);
    }
    for (const step of ["Pick your look", "Startup and updates", "You're all set, Tobias"]) {
      click("Next");
      await heading(step);
    }
    click("Start Mumcord");
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    expect((await harness.api.settingsGet()).setupPreset).toBe("balanced");
  });

  it("applies Maximum privacy, including notify-only updates", async () => {
    renderInWizard(harness, <Wizard />);
    await toSetupStyle();
    fireEvent.click(screen.getByRole("radio", { name: "Maximum privacy" }));
    expect(screen.getByText(/Maximum privacy selected/)).toBeInTheDocument();
    click("Finish setup");
    await heading("You're all set, Tobias");
    click("Start Mumcord");
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    const saved = await harness.api.settingsGet();
    expect(presetValues(saved)).toEqual(withoutCommands(PRESETS.maximumPrivacy));
    expect(saved).toMatchObject({
      setupPreset: "maximumPrivacy",
      updateCheck: "yes",
      updateAutoDownload: false,
    });
    expect((await harness.api.ingameOverlayState()).mode).toBe("off");
  });

  it("opens a step with Edit on the summary and comes back to the summary", async () => {
    renderInWizard(harness, <Wizard />);
    await toSetupStyle();
    click("Finish setup");
    await heading("You're all set, Tobias");
    click("Edit Voice");
    await heading("Sound good?");
    click("Next");
    await heading("You're all set, Tobias");

    // A Custom-only card switches to Custom and comes back as well.
    click("Edit Private messages");
    await heading("Who can message you?");
    fireEvent.click(screen.getByRole("radio", { name: "Nobody" }));
    click("Next");
    await heading("You're all set, Tobias");
    expect(screen.getByText("Nobody")).toBeInTheDocument();
  });

  it("continues with Enter on radio choices (keyboard only)", async () => {
    renderInWizard(harness, <Wizard />);
    await toSetupStyle();
    const balanced = screen.getByRole("radio", { name: "Balanced Recommended" });
    balanced.focus();
    fireEvent.keyDown(balanced, { key: "Enter", code: "Enter" });
    const summary = await heading("You're all set, Tobias");
    await waitFor(() => expect(summary).toHaveFocus());
  });

  it("asks for a push-to-talk key before going on", async () => {
    vi.spyOn(harness.api, "pttCaptureKey").mockImplementation((combo) => Promise.resolve(combo));
    renderInWizard(harness, <Wizard />);
    await toVoice();
    fireEvent.click(screen.getByRole("radio", { name: /Push-to-talk/ }));
    click("Next");
    expect(await screen.findByText(/Choose a push-to-talk key/)).toHaveAttribute("role", "alert");
    const capture = screen.getByRole("button", { name: "Set key" });
    fireEvent.click(capture);
    fireEvent.keyDown(capture, { key: "F8", code: "F8", shiftKey: true });
    expect(await screen.findByText("Shift+F8")).toBeInTheDocument();
    click("Next");
    await heading("How much do you want to share?");
  });

  it("keeps the wizard open when saving fails at the end", async () => {
    renderInWizard(harness, <Wizard />);
    await toSetupStyle();
    click("Finish setup");
    await heading("You're all set, Tobias");
    harness.api.mock.failNext("settingsUpdate", "internal");
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    click("Start Mumcord");
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be saved");
    expect(harness.finishWizard).not.toHaveBeenCalled();
    expect((await harness.api.settingsGet()).wizardCompleted).toBe(false);
  });

  it("finishes even when start at login cannot be set up, and says so", async () => {
    const spoken: string[] = [];
    const unsubscribe = subscribeAnnouncements((a) => spoken.push(a.message));
    renderInWizard(harness, <Wizard />);
    await toSetupStyle();
    fireEvent.click(screen.getByRole("radio", { name: "Custom" }));
    for (const step of [
      "Who can message you?",
      "Connections and content",
      "Make it yours",
      "Pick your look",
      "Startup and updates",
    ]) {
      click("Next");
      await heading(step);
    }
    fireEvent.click(screen.getByRole("switch", { name: "Start Mumcord when I log in" }));
    click("Next");
    await heading("You're all set, Tobias");
    const update = harness.api.settingsUpdate.bind(harness.api);
    vi.spyOn(harness.api, "settingsUpdate").mockImplementation((next) =>
      next.autostart ? Promise.reject(new Error("no login entry")) : update(next),
    );
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    click("Start Mumcord");
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    expect(await harness.api.settingsGet()).toMatchObject({
      wizardCompleted: true,
      autostart: false,
    });
    expect(spoken.join(" ")).toContain("start when you log in");
    unsubscribe();
  });

  it("shows a retry when loading fails", async () => {
    harness.api.mock.failNext("settingsGet", "internal");
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    renderInWizard(harness, <Wizard />);
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be loaded");
    click("Try again");
    expect(await heading("Voice chat that feels like your own place.")).toBeInTheDocument();
  });

  it("still loads when optional lookups fail", async () => {
    harness.api.mock.failNext("pttAvailability", "notImplemented");
    harness.api.mock.failNext("linkHandlerStatus", "notImplemented");
    harness.api.mock.failNext("dndState", "notImplemented");
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    renderInWizard(harness, <Wizard />);
    expect(await heading("Voice chat that feels like your own place.")).toBeInTheDocument();
  });

  it.each(["dark", "light"] as const)("has no axe violations on any step (%s)", async (theme) => {
    harness = await setupWizardTest(theme);
    const { container } = renderInWizard(harness, <Wizard />);
    await heading("Voice chat that feels like your own place.");
    expect(await axeViolations(container)).toEqual([]);
    click("Let's get started");
    await heading("Who are you on Mumble?");
    expect(await axeViolations(container)).toEqual([]);
    fireEvent.change(screen.getByLabelText("Default display name"), { target: { value: "Ana" } });
    click("Next");
    await heading("Sound good?");
    expect(await axeViolations(container)).toEqual([]);
    click("Next");
    await heading("How much do you want to share?");
    expect(await axeViolations(container)).toEqual([]);
    fireEvent.click(screen.getByRole("radio", { name: "Custom" }));
    for (const step of [
      "Who can message you?",
      "Connections and content",
      "Make it yours",
      "Pick your look",
      "Startup and updates",
      "You're all set, Ana",
    ]) {
      click("Next");
      await heading(step);
      expect(await axeViolations(container)).toEqual([]);
    }
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

  /** Presses the primary button until the summary shows (the path depends on the saved values). */
  async function toSummary() {
    for (let guard = 0; guard < 10; guard++) {
      if (screen.queryByRole("heading", { name: /You're all set/ })) return;
      const primary = screen.getByRole("button", {
        name: /^(Let's get started|Next|Finish setup)$/,
      });
      const before = screen.getByRole("heading", { level: 2 });
      fireEvent.click(primary);
      await waitFor(() => {
        expect(screen.getByRole("heading", { level: 2 })).not.toBe(before);
      });
    }
    throw new Error("the summary never showed");
  }

  it("leaves out the identity, says so and links to the identity manager", async () => {
    renderRerun();
    const welcome = await heading("Voice chat that feels like your own place.");
    await waitFor(() => expect(welcome).toHaveFocus());
    const rail = screen.getByRole("navigation", { name: "Setup steps" });
    expect(within(rail).queryByText("Identity")).not.toBeInTheDocument();
    expect(screen.getByText(/Your identity is not changed/)).toBeInTheDocument();
    click("Manage identities");
    expect(onManageIdentities).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "Skip for now" })).toBeInTheDocument();
  });

  it("prefills the saved values", async () => {
    const saved = await harness.api.settingsGet();
    await harness.api.settingsUpdate({ ...saved, talkMode: "pushToTalk", pttKey: "F9" });
    renderRerun();
    await heading("Voice chat that feels like your own place.");
    click("Let's get started");
    await heading("Sound good?");
    expect(screen.getByRole("radio", { name: /Push-to-talk/ })).toBeChecked();
    expect(screen.getByText("F9")).toBeInTheDocument();
  });

  it("writes nothing before Finish and only the changed microphone at Finish", async () => {
    const before = await harness.api.settingsGet();
    const update = vi.spyOn(harness.api, "settingsUpdate");
    const overlay = vi.spyOn(harness.api, "ingameOverlaySetMode");
    const dnd = vi.spyOn(harness.api, "dndSetFollowSystem");
    const identityCreate = vi.spyOn(harness.api, "identityCreate");
    renderRerun();
    await start();
    await heading("Sound good?");
    const mic = screen.getByRole("combobox", { name: "Microphone" });
    await waitFor(() => {
      expect(optionLabels(mic)).toContain("USB Headset");
    });
    chooseOption(mic, "USB Headset");
    await toSummary();
    expect(update).not.toHaveBeenCalled();
    click("Finish");
    await waitFor(() => {
      expect(onClose).toHaveBeenCalledOnce();
    });
    expect(update).toHaveBeenCalledOnce();
    expect(update).toHaveBeenCalledWith({ ...before, inputDevice: "mic-usb" });
    expect(overlay).not.toHaveBeenCalled();
    expect(dnd).not.toHaveBeenCalled();
    expect(identityCreate).not.toHaveBeenCalled();
  });

  it("closes without asking when Cancel drops nothing", async () => {
    renderRerun();
    await heading("Voice chat that feels like your own place.");
    click("Cancel");
    expect(onClose).toHaveBeenCalledOnce();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("asks before Cancel drops a change, and keeps everything on discard", async () => {
    const before = await harness.api.settingsGet();
    const identity = await harness.api.identityGet();
    renderRerun();
    await start();
    await heading("Sound good?");
    await toSummary();
    click("Edit Voice");
    await heading("Sound good?");
    fireEvent.click(screen.getByRole("radio", { name: /Push-to-talk/ }));
    click("Cancel");
    const dialog = await screen.findByRole("alertdialog", { name: "Discard your changes?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Keep editing" }));
    expect(onClose).not.toHaveBeenCalled();
    click("Cancel");
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Discard changes",
      }),
    );
    expect(onClose).toHaveBeenCalledOnce();
    expect(await harness.api.settingsGet()).toEqual(before);
    expect(await harness.api.identityGet()).toEqual(identity);
  });

  it("takes the look preview back on Cancel", async () => {
    const before = await harness.api.settingsGet();
    const { unmount } = renderRerun();
    await heading("Voice chat that feels like your own place.");
    chooseOption(screen.getByRole("combobox", { name: "Display language" }), "Deutsch");
    await waitFor(() => {
      expect(harness.applySettings).toHaveBeenCalled();
    });
    unmount();
    expect(harness.applySettings).toHaveBeenLastCalledWith(before);
  });

  it("keeps the wizard open when saving fails", async () => {
    renderRerun();
    await start();
    await heading("Sound good?");
    fireEvent.click(screen.getByRole("radio", { name: /Voice activation/ }));
    const mic = screen.getByRole("combobox", { name: "Microphone" });
    await waitFor(() => {
      expect(optionLabels(mic)).toContain("USB Headset");
    });
    chooseOption(mic, "USB Headset");
    await toSummary();
    harness.api.mock.failNext("settingsUpdate", "internal");
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    click("Finish");
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be saved");
    expect(onClose).not.toHaveBeenCalled();
  });

  it.each(["dark", "light"] as const)("has no axe violations (%s)", async (theme) => {
    harness = await setupWizardTest(theme, "ready");
    const { container } = renderRerun();
    await heading("Voice chat that feels like your own place.");
    expect(await axeViolations(container)).toEqual([]);
    await toSummary();
    expect(await axeViolations(container)).toEqual([]);
  });
});
