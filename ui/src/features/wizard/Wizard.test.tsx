import { fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setApi } from "../../api";
import { subscribeAnnouncements } from "../../a11y/announcer";
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
  it("finishes with the defaults in three steps and saves wizardCompleted", async () => {
    renderInWizard(harness, <Wizard />);
    const micHeading = await createIdentity();
    await waitFor(() => expect(micHeading).toHaveFocus());
    expect(await harness.api.identityGet()).toEqual({ name: "Tobias", certHash: "selfhash" });

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
  });

  it("announces the step through the focused heading's description", async () => {
    renderInWizard(harness, <Wizard />);
    const heading = await createIdentity();
    expect(heading).toHaveAccessibleDescription("Step 2 of 3");
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
      expect(spoken).toEqual(["Microphone. Step 2 of 3"]);
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
    await harness.api.identityCreate("Ben");
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
    fireEvent.click(
      await screen.findByRole("checkbox", { name: "Open mumble:// links with Mumcord" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    expect(await harness.api.linkHandlerStatus()).toBe(false);
  });

  it.each(["dark", "light"] as const)("has no axe violations on any step (%s)", async (theme) => {
    harness = await setupWizardTest(theme);
    const { container } = renderInWizard(harness, <Wizard />);
    await screen.findByRole("heading", { name: "Your identity" });
    expect(await axeViolations(container)).toEqual([]);
    await createIdentity();
    expect(await axeViolations(container)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "How do you want to talk?" });
    fireEvent.click(screen.getByRole("radio", { name: /Push to talk/ }));
    expect(await axeViolations(container)).toEqual([]);
  });
});
