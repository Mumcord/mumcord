import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../i18n";
import { axeViolations } from "../test/axe";
import { MUMCORD_PRESET, PRESET_THEMES } from "../theme/themeModel";
import { ThemePicker } from "./ThemePicker";
import { swatchOf, type ThemeSwatch } from "./themeSwatch";

const presets = PRESET_THEMES.map((theme) => swatchOf(theme, "dark"));
const night: ThemeSwatch = { ...swatchOf(MUMCORD_PRESET, "light"), id: "custom:n", name: "Night" };

function renderPicker(selectedId = "preset:mumcord") {
  const onSelect = vi.fn();
  const onAdd = vi.fn();
  const onMenu = vi.fn();
  const view = render(
    <ThemePicker
      label="Colour theme"
      presets={presets}
      customThemes={[night]}
      selectedId={selectedId}
      onSelect={onSelect}
      onAdd={onAdd}
      onMenu={onMenu}
    />,
  );
  return { ...view, onSelect, onAdd, onMenu };
}

const circle = (name: string) =>
  screen.getByRole("button", { name: new RegExp(`^${name}, theme`) });

beforeEach(async () => {
  await applyLanguage("en");
});

describe("ThemePicker (FR-2.63)", () => {
  it("shows the presets, then Your themes ending with +, and announces the selected one", async () => {
    const { container } = renderPicker("custom:n");
    const names = screen.getAllByRole("button").map((button) => button.getAttribute("aria-label"));
    expect(names).toEqual([
      "Mumcord, theme",
      "Deep Ocean, theme",
      "Forest, theme",
      "Paper, theme",
      "Night, theme, selected",
      "New theme from the current one",
    ]);
    expect(screen.getByRole("list", { name: "Your themes" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("fills each circle with the background, surface and accent of the version in use", () => {
    renderPicker();
    const { background, surface, accent } = night.colors;
    expect(circle("Night").style.background).toContain("linear-gradient");
    for (const color of [background, surface, accent]) {
      const [r, g, b] = [1, 3, 5].map((at) => parseInt(color.slice(at, at + 2), 16));
      expect(circle("Night").style.background).toContain(
        `rgb(${String(r)}, ${String(g)}, ${String(b)})`,
      );
    }
  });

  it("applies a circle on click, opens the editor with + and the menu with right-click", () => {
    const { onSelect, onAdd, onMenu } = renderPicker();
    fireEvent.click(circle("Forest"));
    expect(onSelect).toHaveBeenCalledWith("preset:forest");
    fireEvent.click(screen.getByRole("button", { name: "New theme from the current one" }));
    expect(onAdd).toHaveBeenCalled();
    fireEvent.contextMenu(circle("Night"), { clientX: 40, clientY: 50 });
    expect(onMenu).toHaveBeenCalledWith("custom:n", { x: 40, y: 50, opener: circle("Night") });
  });

  it("has one Tab stop on the selected circle and moves between circles with the arrow keys", () => {
    renderPicker("preset:forest");
    const tabStops = () => screen.getAllByRole("button").filter((button) => button.tabIndex === 0);
    expect(tabStops()).toEqual([circle("Forest")]);

    circle("Forest").focus();
    fireEvent.keyDown(circle("Forest"), { key: "ArrowRight" });
    expect(circle("Paper")).toHaveFocus();
    fireEvent.keyDown(circle("Paper"), { key: "ArrowDown" });
    expect(circle("Night")).toHaveFocus();
    fireEvent.keyDown(circle("Night"), { key: "ArrowLeft" });
    expect(circle("Paper")).toHaveFocus();
    fireEvent.keyDown(circle("Paper"), { key: "End" });
    expect(screen.getByRole("button", { name: "New theme from the current one" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "ArrowRight" });
    expect(circle("Mumcord")).toHaveFocus();
    expect(tabStops()).toEqual([circle("Mumcord")]);
    // Circles are buttons, so Enter and Space apply the focused one.
    expect(circle("Mumcord").tagName).toBe("BUTTON");
  });
});
