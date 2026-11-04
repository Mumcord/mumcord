import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type AppMenuItem, type MockApi } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { resetPluginStore } from "../pluginStore";
import { PluginsPalette } from "../PluginsPalette";
import { PluginMenuItems } from "./PluginMenuItems";

let api: MockApi;

const entry = (plugin: string, item: string, extra: Partial<AppMenuItem> = {}): AppMenuItem => ({
  plugin,
  pluginName: plugin.toUpperCase(),
  item,
  label: `${plugin} ${item}`,
  icon: null,
  enabled: true,
  reason: null,
  ...extra,
});

beforeEach(async () => {
  await applyLanguage("en");
  resetPluginStore();
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  setApi(undefined);
});

function Menu({ onChosen = () => undefined }: { onChosen?: () => void }) {
  return (
    <div role="menu" aria-label="Apps">
      <PluginMenuItems onChosen={onChosen} />
    </div>
  );
}

describe("PluginMenuItems", () => {
  it("gives icon-less plugins a stable, plugin-specific tile colour", async () => {
    api.appMenu = () =>
      Promise.resolve([entry("brawl", "a"), entry("brawl", "b"), entry("dice", "roll")]);
    const { container } = render(<Menu />);
    await screen.findByRole("menuitem", { name: /dice roll/ });
    const hues = Array.from(container.querySelectorAll<HTMLElement>(".appmenu__icon--tile")).map(
      (tile) => tile.style.getPropertyValue("--appmenu-hue"),
    );
    expect(hues).toHaveLength(3);
    expect(hues[0]).toBe(hues[1]);
    expect(hues[0]).not.toBe(hues[2]);
  });

  it("renders nothing without items", async () => {
    api.appMenu = () => Promise.resolve([]);
    render(<Menu />);
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByText("Plugins")).toBeNull();
  });

  it("shows sub-lines, the disabled reason and the footer, and passes axe", async () => {
    api.appMenu = () =>
      Promise.resolve([
        entry("brawl", "start"),
        entry("dice", "roll", { enabled: false, reason: "Restarting" }),
      ]);
    const { container } = render(<Menu />);
    expect(await screen.findByRole("menuitem", { name: /brawl start/ })).toBeTruthy();
    expect(screen.getByText("BRAWL · Unofficial")).toBeTruthy();
    expect(screen.getByText("Restarting")).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: /dice roll/ }).getAttribute("aria-disabled")).toBe(
      "true",
    );
    expect(screen.getByText(/Manage them in Settings/)).toBeTruthy();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("limits to two per plugin and eight shown, the rest behind More plugins", async () => {
    const items = ["a", "b", "c", "d", "e", "f"].flatMap((p) => [
      entry(p, "1"),
      entry(p, "2"),
      entry(p, "3"),
    ]);
    api.appMenu = () => Promise.resolve(items);
    render(<Menu />);
    await screen.findByRole("menuitem", { name: /a 1/ });
    expect(screen.queryByRole("menuitem", { name: /a 3/ })).toBeNull();
    expect(screen.getAllByRole("menuitem")).toHaveLength(9);
    fireEvent.click(screen.getByRole("menuitem", { name: "More plugins…" }));
    expect(screen.getAllByRole("menuitem")).toHaveLength(12);
  });

  it("activates an enabled item and ignores a disabled one", async () => {
    const activate = vi.fn(() => Promise.resolve(null));
    api.appMenuActivate = activate;
    api.appMenu = () =>
      Promise.resolve([entry("brawl", "start"), entry("dice", "roll", { enabled: false })]);
    const onChosen = vi.fn();
    render(<Menu onChosen={onChosen} />);
    fireEvent.click(await screen.findByRole("menuitem", { name: /dice roll/ }));
    expect(activate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("menuitem", { name: /brawl start/ }));
    expect(activate).toHaveBeenCalledWith("brawl", "start");
    expect(onChosen).toHaveBeenCalledOnce();
  });

  it("refetches on pluginsAppMenuChanged", async () => {
    let list = [entry("brawl", "start")];
    api.appMenu = () => Promise.resolve(list);
    render(<Menu />);
    await screen.findByRole("menuitem", { name: /brawl start/ });
    list = [entry("brawl", "start", { enabled: false, reason: "Busy" })];
    act(() => {
      api.mock.emit("pluginsAppMenuChanged", {});
    });
    await waitFor(() => {
      expect(screen.getByText("Busy")).toBeTruthy();
    });
  });
});

describe("palette", () => {
  it("lists enabled items as plugin: label and runs them", async () => {
    const activate = vi.fn(() => Promise.resolve(null));
    api.appMenuActivate = activate;
    api.appMenu = () =>
      Promise.resolve([entry("brawl", "start"), entry("dice", "roll", { enabled: false })]);
    render(<PluginsPalette />);
    await new Promise((r) => setTimeout(r, 20));
    fireEvent.keyDown(window, { key: "P", ctrlKey: true, shiftKey: true });
    const option = await screen.findByRole("option", { name: "BRAWL: brawl start" });
    expect(screen.queryByRole("option", { name: /dice/ })).toBeNull();
    fireEvent.click(option);
    expect(activate).toHaveBeenCalledWith("brawl", "start");
  });
});
