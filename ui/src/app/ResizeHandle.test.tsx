import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../i18n";
import { axeViolations } from "../test/axe";
import { AppShell, type ShellSlots } from "./AppShell";

const slots: ShellSlots = {
  servers: null,
  channels: <p>Channel tree</p>,
  userPanel: null,
  chatStatus: null,
  chat: <p>Messages</p>,
  members: <p>Member list</p>,
};

const shell = () => document.querySelector(".shell") as HTMLElement;
const handle = (name: string) => screen.getByRole("separator", { name });
const stored = () => JSON.parse(localStorage.getItem("mumcord.columnWidths") ?? "null") as unknown;

beforeEach(async () => {
  await applyLanguage("en");
  localStorage.clear();
  // jsdom has no pointer capture.
  HTMLElement.prototype.setPointerCapture = vi.fn();
  HTMLElement.prototype.releasePointerCapture = vi.fn();
  HTMLElement.prototype.hasPointerCapture = vi.fn(() => true);
});
afterEach(() => {
  localStorage.clear();
});

describe("column resize handles", () => {
  it("are labelled, focusable separators with the width range", async () => {
    const { container } = render(<AppShell slots={slots} />);
    const channels = handle("Resize channel list");
    expect(channels).toHaveAttribute("aria-orientation", "vertical");
    expect(channels).toHaveAttribute("aria-valuenow", "15");
    expect(channels).toHaveAttribute("aria-valuemin", "15");
    expect(channels).toHaveAttribute("aria-valuemax", "28");
    expect(channels).toHaveAttribute("aria-controls", "channels");
    expect(channels).toHaveAttribute("tabindex", "0");
    expect(handle("Resize member list")).toHaveAttribute("aria-controls", "members");
    expect(shell().style.getPropertyValue("--channels-width")).toBe("15rem");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("resizes with the keyboard and remembers the width", () => {
    render(<AppShell slots={slots} />);
    const channels = handle("Resize channel list");
    fireEvent.keyDown(channels, { key: "ArrowRight" });
    fireEvent.keyDown(channels, { key: "ArrowRight" });
    expect(channels).toHaveAttribute("aria-valuenow", "17");
    expect(shell().style.getPropertyValue("--channels-width")).toBe("17rem");
    expect(stored()).toEqual({ channels: 17, members: 15 });

    const members = handle("Resize member list");
    fireEvent.keyDown(members, { key: "End" });
    expect(shell().style.getPropertyValue("--members-width")).toBe("28rem");
    fireEvent.keyDown(members, { key: "ArrowRight" });
    expect(members).toHaveAttribute("aria-valuenow", "27");
    fireEvent.keyDown(members, { key: "Home" });
    expect(members).toHaveAttribute("aria-valuenow", "15");
  });

  it("restores the remembered widths on the next start", () => {
    localStorage.setItem("mumcord.columnWidths", JSON.stringify({ channels: 21, members: 19 }));
    render(<AppShell slots={slots} />);
    expect(handle("Resize channel list")).toHaveAttribute("aria-valuenow", "21");
    expect(shell().style.getPropertyValue("--members-width")).toBe("19rem");
  });

  it("resets to the default width on double-click", () => {
    localStorage.setItem("mumcord.columnWidths", JSON.stringify({ channels: 21, members: 19 }));
    render(<AppShell slots={slots} />);
    fireEvent.doubleClick(handle("Resize channel list"));
    expect(handle("Resize channel list")).toHaveAttribute("aria-valuenow", "15");
    expect(stored()).toEqual({ channels: 15, members: 19 });
  });

  it("follows the pointer while dragging and saves on release", () => {
    render(<AppShell slots={slots} />);
    const channels = handle("Resize channel list");
    fireEvent.pointerDown(channels, { button: 0, clientX: 300, pointerId: 1 });
    expect(document.documentElement.dataset.columnResizing).toBe("channels");
    expect(channels).toHaveFocus();
    vi.useFakeTimers();
    fireEvent.pointerMove(channels, { clientX: 340, pointerId: 1 });
    fireEvent.pointerMove(channels, { clientX: 364, pointerId: 1 });
    act(() => {
      vi.advanceTimersToNextFrame();
    });
    vi.useRealTimers();
    // 64px at the 16px root size.
    expect(channels).toHaveAttribute("aria-valuenow", "19");
    expect(stored()).toBeNull();

    fireEvent.pointerUp(channels, { clientX: 380, pointerId: 1 });
    expect(channels).toHaveAttribute("aria-valuenow", "20");
    expect(stored()).toEqual({ channels: 20, members: 15 });
    expect(document.documentElement.dataset.columnResizing).toBeUndefined();
  });

  it("drags the member list wider towards the left", () => {
    render(<AppShell slots={slots} />);
    const members = handle("Resize member list");
    fireEvent.pointerDown(members, { button: 0, clientX: 1000, pointerId: 1 });
    fireEvent.pointerUp(members, { clientX: 1000 - 5 * 16, pointerId: 1 });
    expect(members).toHaveAttribute("aria-valuenow", "20");
  });
});
