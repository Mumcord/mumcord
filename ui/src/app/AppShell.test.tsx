import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import { AppShell, type ShellSlots } from "./AppShell";

function slots(): ShellSlots {
  return {
    servers: <p>Servers</p>,
    channels: <p>Channels</p>,
    userPanel: <p>User panel</p>,
    chatStatus: null,
    chat: <p>Chat</p>,
    members: <button type="button">Inside members</button>,
  };
}

function stubNarrow(narrow: boolean): void {
  vi.stubGlobal("matchMedia", (query: string) => {
    const listeners = new Set<EventListener>();
    return {
      matches: query.includes("60rem") ? narrow : false,
      media: query,
      onchange: null,
      addEventListener: (_type: string, listener: EventListener) => {
        listeners.add(listener);
      },
      removeEventListener: (_type: string, listener: EventListener) => {
        listeners.delete(listener);
      },
      addListener: (listener: EventListener) => {
        listeners.add(listener);
      },
      removeListener: (listener: EventListener) => {
        listeners.delete(listener);
      },
      dispatchEvent: () => listeners.size > 0,
    };
  });
}

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("AppShell member column", () => {
  it("shows the member column and its hide button by default", () => {
    render(<AppShell slots={slots()} />);
    expect(screen.getByRole("complementary", { name: "Members" })).not.toHaveAttribute("inert");
    expect(screen.getByRole("button", { name: "Hide members" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(screen.queryByRole("button", { name: "Show members" })).not.toBeInTheDocument();
  });

  it("collapses the column, remembers it and moves the focus to the reveal button", async () => {
    render(<AppShell slots={slots()} />);
    const inside = screen.getByRole("button", { name: "Inside members" });
    inside.focus();
    fireEvent.click(screen.getByRole("button", { name: "Hide members" }));

    const reveal = screen.getByRole("button", { name: "Show members" });
    expect(reveal).toHaveAttribute("aria-expanded", "false");
    expect(reveal).toHaveAttribute("aria-controls", "members-content");
    expect(screen.getByRole("complementary", { name: "Members" })).toHaveAttribute("inert");
    expect(localStorage.getItem("mumcord.membersCollapsed")).toBe("true");
    await waitFor(() => {
      expect(reveal).toHaveFocus();
    });

    fireEvent.click(reveal);
    expect(screen.getByRole("button", { name: "Hide members" })).toBeInTheDocument();
    expect(localStorage.getItem("mumcord.membersCollapsed")).toBe("false");
  });

  it("starts collapsed from the stored state", () => {
    localStorage.setItem("mumcord.membersCollapsed", "true");
    render(<AppShell slots={slots()} />);
    expect(screen.getByRole("button", { name: "Show members" })).toBeInTheDocument();
    expect(screen.getByRole("complementary", { name: "Members" })).toHaveAttribute("inert");
  });

  it.each([
    ["Ctrl", { ctrlKey: true }],
    ["Cmd", { metaKey: true }],
  ])("toggles the column with %s+Shift+M", (_name, modifier) => {
    render(<AppShell slots={slots()} />);
    fireEvent.keyDown(window, { key: "m", shiftKey: true, ...modifier });
    expect(screen.getByRole("button", { name: "Show members" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "M", shiftKey: true, ...modifier });
    expect(screen.getByRole("button", { name: "Hide members" })).toBeInTheDocument();
  });

  it("keeps the column collapsed on narrow windows and opens it as an overlay", () => {
    stubNarrow(true);
    render(<AppShell slots={slots()} />);

    const reveal = screen.getByRole("button", { name: "Show members" });
    expect(screen.getByRole("complementary", { name: "Members" })).toHaveAttribute("inert");
    fireEvent.click(reveal);

    expect(screen.getByRole("complementary", { name: "Members" })).not.toHaveAttribute("inert");
    expect(screen.getByRole("complementary", { name: "Members" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Hide members" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );

    fireEvent.click(screen.getByRole("button", { name: "Hide members" }));
    expect(screen.getByRole("button", { name: "Show members" })).toBeInTheDocument();
    expect(screen.getByRole("complementary", { name: "Members" })).toHaveAttribute("inert");
    // Opening the overlay does not change the remembered wide-window preference.
    expect(localStorage.getItem("mumcord.membersCollapsed")).toBeNull();
  });
});
