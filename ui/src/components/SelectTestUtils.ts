import { fireEvent, screen } from "@testing-library/react";

/** Opens the dropdown, returns the labels of its options and closes it again. */
export function optionLabels(combobox: HTMLElement): string[] {
  fireEvent.click(combobox);
  const labels = screen.getAllByRole("option").map((option) => option.textContent);
  fireEvent.keyDown(combobox, { key: "Escape" });
  return labels;
}

/** Picks an option by its visible label with the pointer. */
export function chooseOption(combobox: HTMLElement, label: string): void {
  fireEvent.click(combobox);
  fireEvent.click(screen.getByRole("option", { name: label }));
}
