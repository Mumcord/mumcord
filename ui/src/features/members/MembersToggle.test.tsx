import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "../../i18n";
import { MembersToggle, type MembersToggleProps } from "./MembersToggle";

function renderToggle(change: Partial<MembersToggleProps> = {}) {
  const props: MembersToggleProps = {
    expanded: false,
    controls: "members",
    presence: { count: 6, talkers: [] },
    shortcut: "Ctrl+Shift+M",
    onToggle: vi.fn(),
    ...change,
  };
  render(<MembersToggle {...props} />);
  return { button: screen.getByRole("button"), props };
}

describe("MembersToggle", () => {
  it("shows the member count while the column is hidden", () => {
    const { button } = renderToggle();
    expect(button).toHaveAccessibleName("Show members (6)");
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(button).toHaveAttribute("aria-controls", "members");
    expect(button).toHaveAttribute("aria-keyshortcuts", "Control+Shift+M Meta+Shift+M");
    expect(button).toHaveTextContent("6");
    expect(button.dataset.tooltip).toBe("Show members");
    expect(button.dataset.tooltipShortcut).toBe("Ctrl+Shift+M");
    expect(button.querySelector(".members-toggle__talking")).toBeNull();
    expect(button.dataset.tooltipDetail).toBe("");
  });

  it.each([
    [["Freya"], "Freya is talking"],
    [["Astrid", "Freya"], "Astrid and Freya are talking"],
    [["Astrid", "Freya", "Leif"], "Astrid and 2 others are talking"],
  ])("shows the talking dot and names %j in the tooltip", (talkers, text) => {
    const { button } = renderToggle({ presence: { count: 6, talkers } });
    expect(button.querySelector(".members-toggle__talking")).not.toBeNull();
    expect(button.dataset.tooltipDetail).toBe(text);
  });

  it("shows only the icon, highlighted, while the column is open", () => {
    const { button } = renderToggle({
      expanded: true,
      presence: { count: 6, talkers: ["Freya"] },
    });
    expect(button).toHaveAccessibleName("Hide members");
    expect(button).toHaveAttribute("aria-expanded", "true");
    expect(button).not.toHaveTextContent("6");
    expect(button.querySelector(".members-toggle__talking")).toBeNull();
    expect(button.dataset.tooltip).toBe("Hide members");
  });

  it("has no count in a private chat", () => {
    const { button } = renderToggle({ presence: null });
    expect(button).toHaveAccessibleName("Show members");
    expect(button).toHaveTextContent("");
  });

  it("toggles the column on click", () => {
    const { button, props } = renderToggle();
    fireEvent.click(button);
    expect(props.onToggle).toHaveBeenCalledOnce();
  });
});
