import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setApi } from "../../api";
import { openInBrowser } from "../chat/openLink";
import { PrivacyStep } from "./PrivacyStep";
import { renderInWizard, setupWizardTest } from "./testUtils";
import { Wizard } from "./Wizard";

vi.mock("../chat/openLink", () => ({ openInBrowser: vi.fn() }));

beforeEach(() => {
  vi.mocked(openInBrowser).mockClear();
});

afterEach(() => {
  setApi(undefined);
});

describe("PrivacyStep", () => {
  it("lists the privacy facts under a heading", async () => {
    renderInWizard(
      await setupWizardTest(),
      <PrivacyStep
        step={3}
        total={5}
        p2pMode="directRelay"
        onP2pModeChange={vi.fn()}
        focusOnMount={false}
        onBack={vi.fn()}
        onNext={vi.fn()}
      />,
    );
    expect(screen.getByRole("heading", { name: "Your privacy" })).toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(5);
    expect(screen.getByText(/no telemetry/i)).toBeInTheDocument();
    expect(screen.getByText(/only to the servers you add/i)).toBeInTheDocument();
  });

  it("opens the full privacy statement in the browser", async () => {
    renderInWizard(
      await setupWizardTest(),
      <PrivacyStep
        step={3}
        total={5}
        p2pMode="directRelay"
        onP2pModeChange={vi.fn()}
        focusOnMount={false}
        onBack={vi.fn()}
        onNext={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Full privacy statement" }));
    expect(openInBrowser).toHaveBeenCalledWith(
      "https://github.com/Mumcord/mumcord/blob/main/PRIVACY.md",
    );
  });

  it("offers the five peer-to-peer modes and reports the choice", async () => {
    const onP2pModeChange = vi.fn();
    renderInWizard(
      await setupWizardTest(),
      <PrivacyStep
        step={3}
        total={5}
        p2pMode="directRelay"
        onP2pModeChange={onP2pModeChange}
        focusOnMount={false}
        onBack={vi.fn()}
        onNext={vi.fn()}
      />,
    );
    const modes = screen.getByRole("group", { name: "Peer-to-peer mode" });
    expect(within(modes).getAllByRole("radio")).toHaveLength(5);
    expect(
      within(modes).getByRole("radio", { name: /Direct \+ relay \(direct first\)/ }),
    ).toBeChecked();
    fireEvent.click(within(modes).getByRole("radio", { name: "Relay only" }));
    expect(onP2pModeChange).toHaveBeenCalledWith("relayOnly");
  });

  it("saves the chosen peer-to-peer mode with the final step on first start", async () => {
    const harness = await setupWizardTest();
    renderInWizard(harness, <Wizard />);
    fireEvent.change(await screen.findByLabelText("Name"), { target: { value: "Kim" } });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "Microphone" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "Your privacy" });
    fireEvent.click(screen.getByRole("radio", { name: "Direct only" }));
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "New versions" });
    expect((await harness.api.settingsGet()).p2pMode).toBe("directRelay");
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    await screen.findByRole("heading", { name: "Your privacy" });
    expect(screen.getByRole("radio", { name: "Direct only" })).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "New versions" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "How do you want to talk?" });
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => {
      expect(harness.finishWizard).toHaveBeenCalledOnce();
    });
    expect((await harness.api.settingsGet()).p2pMode).toBe("directOnly");
  });

  it("writes a changed peer-to-peer mode in the rerun, and nothing when it is unchanged", async () => {
    const harness = await setupWizardTest("dark", "ready");
    // A decided update hint, so finishing with no change has nothing to write.
    await harness.api.settingsUpdate({ ...(await harness.api.settingsGet()), updateCheck: "no" });
    const update = vi.spyOn(harness.api, "settingsUpdate");
    const onClose = vi.fn<() => void>();
    const { unmount } = renderInWizard(harness, <Wizard mode="rerun" onClose={onClose} />);
    await finishRerunVia(undefined);
    await waitFor(() => {
      expect(onClose).toHaveBeenCalledOnce();
    });
    expect(update).not.toHaveBeenCalled();
    unmount();

    renderInWizard(harness, <Wizard mode="rerun" onClose={onClose} />);
    await finishRerunVia("Off");
    await waitFor(() => {
      expect(onClose).toHaveBeenCalledTimes(2);
    });
    expect(update).toHaveBeenCalledOnce();
    expect((await harness.api.settingsGet()).p2pMode).toBe("off");
  });

  it("comes before the link-preview choice in the wizard", async () => {
    renderInWizard(await setupWizardTest(), <Wizard />);
    fireEvent.change(await screen.findByLabelText("Name"), { target: { value: "Kim" } });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "Microphone" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "Your privacy" });
    expect(screen.queryByRole("checkbox", { name: "Show link previews and embeds" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "New versions" });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByRole("heading", { name: "How do you want to talk?" });
    expect(screen.getByRole("checkbox", { name: "Show link previews and embeds" })).toBeChecked();
  });
});

/** Rerun from the microphone step to Finish, choosing `mode` on the privacy step when given. */
async function finishRerunVia(mode: string | undefined) {
  await screen.findByRole("heading", { name: "Microphone" });
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByRole("heading", { name: "Your privacy" });
  if (mode !== undefined) fireEvent.click(screen.getByRole("radio", { name: mode }));
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByRole("heading", { name: "New versions" });
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByRole("heading", { name: "How do you want to talk?" });
  fireEvent.click(screen.getByRole("button", { name: "Finish" }));
}
