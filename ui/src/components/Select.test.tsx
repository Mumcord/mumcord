import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
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
