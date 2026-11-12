import { act, fireEvent, render, screen } from "@testing-library/react";
import { useRef, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../i18n";
import type { WizardStepId } from "./wizardFlow";
import { WizardGuide } from "./WizardGuide";

function Host({ step, children }: { step: WizardStepId; children?: ReactNode }) {
  const root = useRef<HTMLDivElement>(null);
  return (
    <div>
      <div ref={root}>{children}</div>
      <WizardGuide step={step} stepRoot={root} />
    </div>
  );
}

const MIC_FIELD = (
  <div className="wizard-voice__devices">
    <select aria-label="Microphone" data-testid="mic" />
  </div>
);

/** Fake layout: Aaron's column on the right, the microphone field wherever the test puts it. */
let micBox = new DOMRect(100, 300, 400, 40);
const COLUMN = new DOMRect(800, 100, 240, 600);

beforeEach(async () => {
  await applyLanguage("en");
  micBox = new DOMRect(100, 300, 400, 40);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
    this: HTMLElement,
  ) {
    if (this.classList.contains("wizard-guide")) return COLUMN;
    if (this.dataset.testid === "mic") return micBox;
    return new DOMRect();
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("WizardGuide", () => {
  it("shows one hint per step", () => {
    const { rerender } = render(<Host step="welcome" />);
    expect(screen.getByRole("complementary", { name: "Tip from Aaron" })).toHaveTextContent(
      "I'll walk you through the setup",
    );
    rerender(<Host step="voice">{MIC_FIELD}</Host>);
    expect(screen.getByText("Pick your microphone")).toBeInTheDocument();
    expect(screen.getByText(/Choose the mic you talk into/)).toBeInTheDocument();
  });

  it("makes the field he points at glow until he leaves or hides", () => {
    const { rerender } = render(<Host step="voice">{MIC_FIELD}</Host>);
    expect(screen.getByTestId("mic")).toHaveAttribute("data-guide-target");

    fireEvent.click(screen.getByRole("button", { name: "Hide Aaron" }));
    expect(screen.getByTestId("mic")).not.toHaveAttribute("data-guide-target");

    fireEvent.click(screen.getByRole("button", { name: "Show Aaron" }));
    expect(screen.getByTestId("mic")).toHaveAttribute("data-guide-target");

    rerender(<Host step="welcome">{MIC_FIELD}</Host>);
    expect(screen.getByTestId("mic")).not.toHaveAttribute("data-guide-target");
  });

  it("steps out of his column to stand right next to the field", () => {
    render(<Host step="voice">{MIC_FIELD}</Host>);
    // The microphone ends at 500, his column starts at 800.
    expect(screen.getByTestId("aaron").parentElement?.style.transform).toMatch(
      /^translate\(-284px, /,
    );
  });

  it("keeps Aaron himself hidden from screen readers", () => {
    render(<Host step="welcome" />);
    expect(screen.getByTestId("aaron")).toHaveAttribute("aria-hidden", "true");
  });

  it("stands idle on a step without a target", () => {
    render(<Host step="welcome" />);
    expect(screen.getByTestId("aaron")).toHaveAttribute("data-pose", "idle");
  });

  it("points at the microphone and follows it on resize", () => {
    render(<Host step="voice">{MIC_FIELD}</Host>);
    expect(screen.getByTestId("aaron")).toHaveAttribute("data-pose", "point-left");

    micBox = new DOMRect(850, 0, 100, 40);
    act(() => {
      fireEvent(window, new Event("resize"));
    });
    expect(screen.getByTestId("aaron")).toHaveAttribute("data-pose", "point-up");
  });

  it("hides for the rest of the wizard and comes back on request", () => {
    const { rerender } = render(<Host step="welcome" />);

    fireEvent.click(screen.getByRole("button", { name: "Hide Aaron" }));
    expect(screen.queryByTestId("aaron")).not.toBeInTheDocument();
    const show = screen.getByRole("button", { name: "Show Aaron" });
    expect(show).toHaveFocus();

    rerender(<Host step="voice">{MIC_FIELD}</Host>);
    expect(screen.queryByTestId("aaron")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Show Aaron" }));
    expect(screen.getByTestId("aaron")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Hide Aaron" })).toHaveFocus();
  });
});
