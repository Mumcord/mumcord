import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { axeViolations } from "../../test/axe";
import { adminEntry } from "../admin/adminMenu";
import { ServerMenu, type MenuItem } from "./ServerMenu";

afterEach(cleanup);

function renderMenu(onBanList = vi.fn(), onClose = vi.fn()) {
  const admin = adminEntry("Admin", [{ id: "banList", label: "Ban list…", onSelect: onBanList }]);
  const items: MenuItem[] = [{ id: "info", label: "Server info", onSelect: vi.fn() }];
  if (admin) items.push(admin);
  const view = render(
    <ServerMenu label="Server" items={items} position={{ x: 0, y: 0 }} onClose={onClose} />,
  );
  return { view, onBanList, onClose };
}

describe("ServerMenu sub-menus", () => {
  it("opens the sub-menu on click and runs the nested item", () => {
    const { onBanList } = renderMenu();
    const admin = screen.getByRole("menuitem", { name: /Admin/ });
    expect(admin).toHaveAttribute("aria-haspopup", "menu");
    expect(admin).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(admin);
    expect(admin).toHaveAttribute("aria-expanded", "true");
    const submenu = screen.getByRole("menu", { name: "Admin" });
    fireEvent.click(within(submenu).getByRole("menuitem", { name: "Ban list…" }));
    expect(onBanList).toHaveBeenCalledOnce();
  });

  it("opens with → and closes with ← or Escape, returning focus to the opener", () => {
    const { onClose } = renderMenu();
    const admin = screen.getByRole("menuitem", { name: /Admin/ });
    admin.focus();

    fireEvent.keyDown(admin, { key: "ArrowRight" });
    const banList = screen.getByRole("menuitem", { name: "Ban list…" });
    expect(banList).toHaveFocus();
    fireEvent.keyDown(banList, { key: "ArrowLeft" });
    expect(screen.queryByRole("menu", { name: "Admin" })).not.toBeInTheDocument();
    expect(admin).toHaveFocus();

    fireEvent.keyDown(admin, { key: "ArrowRight" });
    fireEvent.keyDown(screen.getByRole("menuitem", { name: "Ban list…" }), { key: "Escape" });
    expect(screen.queryByRole("menu", { name: "Admin" })).not.toBeInTheDocument();
    expect(admin).toHaveFocus();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("arrow keys in the parent skip the sub-menu's items", () => {
    renderMenu();
    const info = screen.getByRole("menuitem", { name: "Server info" });
    const admin = screen.getByRole("menuitem", { name: /Admin/ });
    fireEvent.click(admin);
    admin.focus();
    fireEvent.keyDown(admin, { key: "ArrowDown" });
    expect(info).toHaveFocus();
  });

  it("has no accessibility violations with the sub-menu open", async () => {
    const { view } = renderMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: /Admin/ }));
    expect(await axeViolations(view.container)).toEqual([]);
  });
});

describe("adminEntry", () => {
  const item = (id: string, danger = false): MenuItem => ({
    id,
    label: id,
    danger,
    onSelect: vi.fn(),
  });

  it("is left out when the user may do nothing", () => {
    expect(adminEntry("Admin", [])).toBeNull();
  });

  it("puts red items last behind one divider", () => {
    const entry = adminEntry("Admin", [item("ban", true), item("edit"), item("kick", true)]);
    expect(entry?.submenu?.map((i) => [i.id, i.separatorBefore === true])).toEqual([
      ["edit", false],
      ["ban", true],
      ["kick", false],
    ]);
  });

  it("draws no divider when only red items are allowed", () => {
    const entry = adminEntry("Admin", [item("ban", true)]);
    expect(entry?.submenu?.[0]?.separatorBefore).not.toBe(true);
  });
});
