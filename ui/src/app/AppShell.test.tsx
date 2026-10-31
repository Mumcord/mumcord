import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import { MembersToggle } from "../features/members";
import { AppShell, type ShellSlots } from "./AppShell";
import { useMembersColumn } from "./membersCollapse";

/** The chat header's members button, as the chat mount wires it. */
function HeaderButton() {
  const column = useMembersColumn();
  if (column === null) return null;
  return (
    <MembersToggle
      expanded={column.shown}
      controls={column.controls}
      presence={{ count: 3, talkers: [] }}
      shortcut="Ctrl+Shift+M"
      onToggle={column.toggle}
      buttonRef={column.buttonRef}
    />
  );
}

function slots(): ShellSlots {
  return {
    servers: <p>Servers</p>,
    channels: <p>Channels</p>,
    userPanel: <p>User panel</p>,
    chatStatus: null,
    chat: (
      <header className="chat-header">
        <HeaderButton />
      </header>
    ),
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
  const column = () => screen.getByRole("complementary", { name: "Members" });

  it("shows the member column by default with the header button expanded", () => {
    render(<AppShell slots={slots()} />);
    expect(column()).not.toHaveAttribute("inert");
    const button = screen.getByRole("button", { name: "Hide members" });
    expect(button).toHaveAttribute("aria-expanded", "true");
    expect(button).toHaveAttribute("aria-controls", "members");
    expect(column()).toHaveAttribute("id", "members");
  });

  it("hides the column, remembers it and moves the focus to the header button", async () => {
    render(<AppShell slots={slots()} />);
    screen.getByRole("button", { name: "Inside members" }).focus();
    fireEvent.click(screen.getByRole("button", { name: "Hide members" }));

    const button = screen.getByRole("button", { name: "Show members (3)" });
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(column()).toHaveAttribute("inert");
    expect(localStorage.getItem("mumcord.membersCollapsed")).toBe("true");
    await waitFor(() => {
      expect(button).toHaveFocus();
    });

    fireEvent.click(button);
    expect(screen.getByRole("button", { name: "Hide members" })).toBeInTheDocument();
    expect(localStorage.getItem("mumcord.membersCollapsed")).toBe("false");
  });

  it("starts hidden from the stored state", () => {
    localStorage.setItem("mumcord.membersCollapsed", "true");
    render(<AppShell slots={slots()} />);
    expect(screen.getByRole("button", { name: "Show members (3)" })).toBeInTheDocument();
    expect(column()).toHaveAttribute("inert");
  });

  it.each([
    ["Ctrl", { ctrlKey: true }],
    ["Cmd", { metaKey: true }],
  ])("toggles the column with %s+Shift+M", (_name, modifier) => {
    render(<AppShell slots={slots()} />);
    fireEvent.keyDown(window, { key: "m", shiftKey: true, ...modifier });
    expect(screen.getByRole("button", { name: "Show members (3)" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "M", shiftKey: true, ...modifier });
    expect(screen.getByRole("button", { name: "Hide members" })).toBeInTheDocument();
  });

  it("ignores the shortcut while the shell is inert", () => {
    render(<AppShell slots={slots()} inert />);
    fireEvent.keyDown(window, { key: "m", shiftKey: true, ctrlKey: true });
    expect(localStorage.getItem("mumcord.membersCollapsed")).toBeNull();
  });

  it("hides the column on narrow windows and opens it as an overlay below the header", () => {
    stubNarrow(true);
    const { container } = render(<AppShell slots={slots()} />);

    expect(column()).toHaveAttribute("inert");
    fireEvent.click(screen.getByRole("button", { name: "Show members (3)" }));

    expect(column()).not.toHaveAttribute("inert");
    expect(container.querySelector(".shell")).toHaveAttribute("data-members-open", "true");
    expect(screen.getByRole("button", { name: "Hide members" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );

    fireEvent.click(screen.getByRole("button", { name: "Hide members" }));
    expect(column()).toHaveAttribute("inert");
    // Opening the overlay does not change the remembered wide-window preference.
    expect(localStorage.getItem("mumcord.membersCollapsed")).toBeNull();
  });

  it("closes the overlay with Escape and gives the focus back to the header button", async () => {
    stubNarrow(true);
    render(<AppShell slots={slots()} />);
    fireEvent.click(screen.getByRole("button", { name: "Show members (3)" }));
    screen.getByRole("button", { name: "Inside members" }).focus();

    fireEvent.keyDown(window, { key: "Escape" });

    expect(column()).toHaveAttribute("inert");
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Show members (3)" })).toHaveFocus();
    });
  });

  it("closes the overlay from the scrim", () => {
    stubNarrow(true);
    const { container } = render(<AppShell slots={slots()} />);
    fireEvent.click(screen.getByRole("button", { name: "Show members (3)" }));

    const scrim = container.querySelector(".shell__members-scrim");
    expect(scrim).not.toBeNull();
    if (scrim) fireEvent.click(scrim);

    expect(column()).toHaveAttribute("inert");
    expect(container.querySelector(".shell__members-scrim")).toBeNull();
  });

  it("leaves Escape alone while the column is a regular column", () => {
    render(<AppShell slots={slots()} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(column()).not.toHaveAttribute("inert");
  });
});
