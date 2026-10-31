import { fireEvent, screen } from "@testing-library/react";
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
      <PrivacyStep step={3} total={5} focusOnMount={false} onBack={vi.fn()} onNext={vi.fn()} />,
    );
    expect(screen.getByRole("heading", { name: "Your privacy" })).toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(5);
    expect(screen.getByText(/no telemetry/i)).toBeInTheDocument();
    expect(screen.getByText(/only to the servers you add/i)).toBeInTheDocument();
  });

  it("opens the full privacy statement in the browser", async () => {
    renderInWizard(
      await setupWizardTest(),
      <PrivacyStep step={3} total={5} focusOnMount={false} onBack={vi.fn()} onNext={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Full privacy statement" }));
    expect(openInBrowser).toHaveBeenCalledWith(
      "https://github.com/Mumcord/mumcord/blob/main/PRIVACY.md",
    );
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
