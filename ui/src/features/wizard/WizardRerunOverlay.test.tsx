import { fireEvent, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setApi } from "../../api";
import { axeViolations } from "../../test/axe";
import { renderInWizard, setupWizardTest, type Harness } from "./testUtils";
import { WizardRerunOverlay } from "./WizardRerunOverlay";

let harness: Harness;

beforeEach(async () => {
  harness = await setupWizardTest("dark", "ready");
});

afterEach(() => {
  setApi(undefined);
});

/** About's button plus the wizard it opens, as the app mounts them (#149). */
function Opener() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        onClick={() => {
          setOpen(true);
        }}
      >
        Run setup wizard again
      </button>
      {open && (
        <WizardRerunOverlay
          onClose={() => {
            setOpen(false);
          }}
          onManageIdentities={vi.fn()}
        />
      )}
    </>
  );
}

describe("WizardRerunOverlay", () => {
  it("opens as a modal dialog titled Setup wizard over the settings", async () => {
    renderInWizard(harness, <Opener />);
    fireEvent.click(screen.getByRole("button", { name: "Run setup wizard again" }));
    const dialog = await screen.findByRole("dialog", { name: "Setup wizard" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(await axeViolations(dialog)).toEqual([]);
  });

  it("moves focus to the wizard heading and back to the button on close", async () => {
    renderInWizard(harness, <Opener />);
    const run = screen.getByRole("button", { name: "Run setup wizard again" });
    run.focus();
    fireEvent.click(run);
    const heading = await screen.findByRole("heading", { name: "Microphone" });
    await waitFor(() => expect(heading).toHaveFocus());
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(run).toHaveFocus());
    expect(screen.queryByRole("dialog", { name: "Setup wizard" })).not.toBeInTheDocument();
  });
});
