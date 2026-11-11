import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { listHost, placeList } from "./selectPlacement";
import { Select } from "./Select";

const OPTIONS = [
  { value: "a", label: "Apple" },
  { value: "b", label: "Banana" },
  { value: "c", label: "Cherry" },
];

function setup(value = "a") {
  const onChange = vi.fn();
  render(
    <>
      <label htmlFor="fruit">Fruit</label>
      <Select id="fruit" value={value} options={OPTIONS} onChange={onChange} />
    </>,
  );
  return { onChange, box: screen.getByRole("combobox", { name: "Fruit" }) };
}

describe("Select", () => {
  it("is named by its label and shows the selected option", () => {
    const { box } = setup("b");
    expect(box).toHaveTextContent("Banana");
    expect(box).toHaveAttribute("aria-expanded", "false");
  });

  it("opens with Enter, moves with the arrows and chooses with Enter", () => {
    const { box, onChange } = setup();
    fireEvent.keyDown(box, { key: "Enter" });
    expect(box).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    fireEvent.keyDown(box, { key: "ArrowDown" });
    fireEvent.keyDown(box, { key: "ArrowDown" });
    expect(box).toHaveAttribute(
      "aria-activedescendant",
      screen.getByRole("option", { name: "Cherry" }).id,
    );
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onChange).toHaveBeenCalledWith("c");
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("opens with Space and ArrowDown, and jumps with Home and End", () => {
    const { box, onChange } = setup("b");
    fireEvent.keyDown(box, { key: " " });
    expect(box).toHaveAttribute("aria-expanded", "true");
    fireEvent.keyDown(box, { key: "End" });
    fireEvent.keyDown(box, { key: " " });
    expect(onChange).toHaveBeenLastCalledWith("c");
    fireEvent.keyDown(box, { key: "ArrowDown" });
    fireEvent.keyDown(box, { key: "Home" });
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onChange).toHaveBeenLastCalledWith("a");
  });

  it("closes with Escape without choosing and keeps the key from the dialog around it", () => {
    const { box, onChange } = setup();
    const outer = vi.fn();
    document.addEventListener("keydown", outer);
    fireEvent.keyDown(box, { key: "ArrowDown" });
    fireEvent.keyDown(box, { key: "ArrowDown" });
    fireEvent.keyDown(box, { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(onChange).not.toHaveBeenCalled();
    expect(outer).not.toHaveBeenCalled();
    fireEvent.keyDown(box, { key: "Escape" });
    expect(outer).toHaveBeenCalledOnce();
    document.removeEventListener("keydown", outer);
  });

  it("selects by typing the first letters", () => {
    const { box, onChange } = setup();
    fireEvent.keyDown(box, { key: "c" });
    expect(onChange).toHaveBeenCalledWith("c");
  });

  it("marks the selected option and picks one with the pointer", () => {
    const { box, onChange } = setup("b");
    fireEvent.click(box);
    expect(screen.getByRole("option", { name: "Banana" })).toHaveAttribute("aria-selected", "true");
    fireEvent.click(screen.getByRole("option", { name: "Apple" }));
    expect(onChange).toHaveBeenCalledWith("a");
  });

  it("does not open when disabled", () => {
    const onChange = vi.fn();
    render(<Select ariaLabel="Fruit" value="a" options={OPTIONS} onChange={onChange} disabled />);
    const box = screen.getByRole("combobox", { name: "Fruit" });
    fireEvent.keyDown(box, { key: "Enter" });
    fireEvent.click(box);
    expect(screen.queryByRole("listbox")).toBeNull();
  });
});

describe("Select list placement", () => {
  /** A button rect as `getBoundingClientRect` would report it. */
  const rect = (top: number, height = 32) =>
    ({ top, bottom: top + height, left: 40, width: 200 }) as DOMRect;

  it("opens below the button when the window has room", () => {
    const place = placeList(rect(100), 800);
    expect(place.above).toBeNull();
    expect(place.top).toBe(136);
  });

  it("opens upwards near the bottom edge, so the list is never cut off", () => {
    // 40 px below, 700 px above: a clipped list is what this prevents (FreeBSD card bug).
    const place = placeList(rect(700), 800);
    expect(place.above).not.toBeNull();
    expect(place.maxHeight).toBeGreaterThan(100);
  });

  it("lives outside the card that holds the button, so no ancestor can clip it", () => {
    const onChange = vi.fn();
    const { container } = render(
      <div style={{ overflow: "hidden" }}>
        <Select id="fruit" value="a" options={OPTIONS} onChange={onChange} />
      </div>,
    );
    fireEvent.click(screen.getByRole("combobox"));
    const list = screen.getByRole("listbox");
    expect(container.contains(list)).toBe(false);
    expect(document.body.contains(list)).toBe(true);
    // The pointer still picks an option through the portal.
    fireEvent.click(screen.getByRole("option", { name: "Cherry" }));
    expect(onChange).toHaveBeenCalledWith("c");
  });

  it("stays inside an open modal dialog, whose top layer the body is below", () => {
    const dialog = document.createElement("dialog");
    dialog.setAttribute("open", "");
    document.body.append(dialog);
    try {
      const trigger = document.createElement("button");
      dialog.append(trigger);
      expect(listHost(trigger)).toBe(dialog);
      expect(listHost(document.createElement("button"))).toBe(document.body);
      expect(listHost(null)).toBe(document.body);
    } finally {
      dialog.remove();
    }
  });
});
