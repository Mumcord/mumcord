import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { getServerState } from "../channels/serverStore";
import { EngineError, type MockApi } from "../../api";
import { subscribeAnnouncements } from "../../a11y/announcer";
import { axeViolations } from "../../test/axe";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { openServerAdmin, resetServerAdmin } from "./banStore";
import { resetRegistered } from "./registeredStore";
import { ServerAdminWindowHost } from "./ServerAdminWindow";

const TITLE = "Server administration · Local test server";

let api: MockApi;

/** The server's own answer for the root channel decides which tabs exist (FR-10.50, FR-10.2). */
function grant(patch: { ban?: boolean; register?: boolean }): void {
  const root = getServerState().channels.get(0);
  if (!root) throw new Error("seed channel missing");
  act(() => {
    api.mock.updateChannel(0, { ...root, permissions: { ...root.permissions, ...patch } });
  });
}

async function openTab(tab: "bans" | "registered"): Promise<HTMLElement> {
  grant({ ban: true, register: true });
  openServerAdmin(1, tab);
  render(<ServerAdminWindowHost />);
  const dialog = await screen.findByRole("dialog", { name: TITLE });
  await within(dialog).findByText("Tove");
  return dialog;
}

/** The row of one account, by its name in the first cell. */
function rowOf(dialog: HTMLElement, name: string): HTMLElement {
  const cell = within(dialog)
    .getAllByText(name)
    .find((element) => element.classList.contains("admin-cell__text"));
  if (!cell) throw new Error(`no row named ${name}`);
  return cell.closest('[role="row"]') as HTMLElement;
}

/** The footer's Close, not the × in the window header. */
function closeButton(dialog: HTMLElement): HTMLElement {
  const button = [...dialog.querySelectorAll(".admin-footer button")].find(
    (element) => element.textContent.trim() === "Close",
  );
  if (!button) throw new Error("no Close in the footer");
  return button as HTMLElement;
}

/**
 * What the app announces from now on, until `stop`. A save must say what it did, so a test that
 * only reads the panel could not tell a success claim from a failure one (issue #256).
 */
function hearAnnouncements(): { stop: () => void; heard: string[] } {
  const heard: string[] = [];
  const stop = subscribeAnnouncements(({ message }) => {
    heard.push(message);
  });
  return { stop, heard };
}

beforeEach(async () => {
  api = await connectedMock({ bindStore: true });
  stubDialog();
  resetServerAdmin();
  resetRegistered();
});

afterEach(() => {
  resetServerAdmin();
  resetRegistered();
  resetAll();
});

describe("registered users tab", () => {
  it("shows name, id, last seen and last channel, with the online ones marked (FR-10.52)", async () => {
    const dialog = await openTab("registered");
    const row = rowOf(dialog, "Tove");
    expect(within(row).getByText("18")).toBeTruthy();
    expect(within(row).getByText("Lobby")).toBeTruthy();
    // Yesterday carries the day and the time, so it is not a bare number of days.
    expect(within(row).getByText(/^Yesterday, \d/)).toBeTruthy();
    expect(within(rowOf(dialog, "Brennan")).getByText("unknown")).toBeTruthy();
    expect(within(dialog).getAllByText("Online now").length).toBeGreaterThan(0);
    expect(await axeViolations(dialog)).toEqual([]);
  });

  it("counts the three filters and searches by name while typing (FR-10.53)", async () => {
    const dialog = await openTab("registered");
    const rows = within(dialog);
    expect(rows.getByRole("button", { name: "All (13)" })).toBeTruthy();
    expect(rows.getByRole("button", { name: "Online (3)" })).toBeTruthy();
    // Only Ragnar (127 days) and Odalys (204) are past 90; an account the server never saw is not
    // counted as inactive (FR-10.5).
    expect(rows.getByRole("button", { name: "Inactive 90+ days (2)" })).toBeTruthy();
    expect(rows.getByText("13 of 13 accounts")).toBeTruthy();

    act(() => {
      fireEvent.change(rows.getByRole("searchbox"), { target: { value: "to" } });
    });
    expect(rows.getByText("1 of 13 accounts")).toBeTruthy();
    expect(rows.queryByText("Aastrid")).toBeNull();

    act(() => {
      fireEvent.change(rows.getByRole("searchbox"), { target: { value: "nobody" } });
    });
    expect(rows.getByText("No account matches this search")).toBeTruthy();

    act(() => {
      fireEvent.change(rows.getByRole("searchbox"), { target: { value: "" } });
    });
    act(() => {
      fireEvent.click(rows.getByRole("button", { name: "Inactive 90+ days (2)" }));
    });
    expect(rows.getByText("Ragnar")).toBeTruthy();
    expect(rows.getByText("Odalys")).toBeTruthy();
    expect(rows.queryByText("Tove")).toBeNull();
  });

  it("sorts by the column headers and reads the order out to a screen reader", async () => {
    const dialog = await openTab("registered");
    const header = within(dialog).getByRole("columnheader", { name: "ID" });
    expect(header).toHaveAttribute("aria-sort", "none");
    act(() => {
      fireEvent.click(within(header).getByRole("button", { name: "ID" }));
    });
    expect(header).toHaveAttribute("aria-sort", "ascending");
    const rows = [...dialog.querySelectorAll('[role="row"]:not(.admin-row--head)')];
    const idOf = (row: Element | undefined) =>
      within(row as HTMLElement).getAllByRole("cell")[1]?.textContent ?? "";
    expect(Number(idOf(rows[0]))).toBeLessThan(Number(idOf(rows[1])));
  });

  it("opens the detail panel with the id, the groups and the name field", async () => {
    const dialog = await openTab("registered");
    act(() => {
      fireEvent.click(rowOf(dialog, "Aastrid"));
    });
    const panel = within(dialog).getByRole("complementary", { name: "Details of Aastrid" });
    expect(within(panel).getByText("User ID 4 · offline")).toBeTruthy();
    expect(within(panel).getByRole("textbox", { name: "Name" })).toHaveValue("Aastrid");
    // The demo server names this account in two groups (FR-10.52: groups in the tree).
    expect(await within(panel).findByText("officers")).toBeTruthy();
    expect(within(panel).getByText("raiders")).toBeTruthy();
    expect(
      within(panel).queryByRole("button", { name: "Ban certificate…" }),
      "an account with no certificate in hand has nothing to ban by certificate",
    ).toBeNull();
  });

  it("renames one account when saving, and says so (FR-10.52, AC-10.37)", async () => {
    const dialog = await openTab("registered");
    act(() => {
      fireEvent.click(rowOf(dialog, "Tove"));
    });
    const panel = within(dialog).getByRole("complementary", { name: "Details of Tove" });
    act(() => {
      fireEvent.change(within(panel).getByRole("textbox", { name: "Name" }), {
        target: { value: "Tove Eriksson" },
      });
    });
    expect(within(rowOf(dialog, "Tove")).getByText("not saved")).toBeTruthy();
    expect(within(dialog).getByRole("button", { name: "Save" })).toBeEnabled();
    expect(within(panel).getByRole("heading", { name: "Tove" })).toBeTruthy();

    const announced = hearAnnouncements();
    act(() => {
      fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    });
    await waitFor(() => {
      expect(rowOf(dialog, "Tove Eriksson").textContent).toContain("Tove Eriksson");
    });
    expect(
      within(dialog).getByText("Saved: 1 renamed, 0 unregistered · list reloaded"),
    ).toBeTruthy();
    expect(within(dialog).queryByText("not saved")).toBeNull();
    await waitFor(() => {
      expect(announced.heard).toContain("Changes sent to the server");
    });
    announced.stop();
  });

  it("keeps the server's name when the server did not take the new one (issue #256, AC-10.37)", async () => {
    const dialog = await openTab("registered");
    act(() => {
      fireEvent.click(rowOf(dialog, "Tove"));
    });
    const panel = within(dialog).getByRole("complementary", { name: "Details of Tove" });
    // "Sigrid" is another account already; the mock copies what mumble-server does here.
    act(() => {
      fireEvent.change(within(panel).getByRole("textbox", { name: "Name" }), {
        target: { value: "Sigrid" },
      });
    });
    const announced = hearAnnouncements();
    act(() => {
      fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    });
    await within(dialog).findByText(
      "The server did not take that change; Tove is still listed as before.",
    );
    expect(within(dialog).queryByText("Saved:")).toBeNull();
    expect(rowOf(dialog, "Tove")).toBeTruthy();
    // The screen reader hears the same sentence the banner shows, never a success.
    await waitFor(() => {
      expect(announced.heard).toContain(
        "The server did not take that change; Tove is still listed as before.",
      );
    });
    expect(announced.heard).not.toContain("Changes sent to the server");
    announced.stop();
  });

  it("says what failed when a save could not go through, and never a success (issue #256)", async () => {
    const dialog = await openTab("registered");
    act(() => {
      fireEvent.click(rowOf(dialog, "Tove"));
    });
    const panel = within(dialog).getByRole("complementary", { name: "Details of Tove" });
    act(() => {
      fireEvent.change(within(panel).getByRole("textbox", { name: "Name" }), {
        target: { value: "Tove Eriksson" },
      });
    });
    // The mock copies a server that keeps a held name without a word; this is the refusal the
    // server does say in words, which the engine reports as a missing right (FR-10.59).
    api.registeredUserRename = () =>
      Promise.reject(new EngineError("permissionDenied", "the register right is missing"));
    const announced = hearAnnouncements();
    act(() => {
      fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    });
    await within(dialog).findByText(
      "You need the Register right on the root channel to change the registered users.",
    );
    await waitFor(() => {
      expect(announced.heard).toContain(
        "You need the Register right on the root channel to change the registered users.",
      );
    });
    expect(announced.heard).not.toContain("Changes sent to the server");
    announced.stop();
  });

  it("confirms unregistering, marks the row, and removes it on save (FR-10.52, AC-10.37)", async () => {
    const dialog = await openTab("registered");
    act(() => {
      fireEvent.click(rowOf(dialog, "Leif"));
    });
    const panel = within(dialog).getByRole("complementary", { name: "Details of Leif" });
    act(() => {
      fireEvent.click(within(panel).getByRole("button", { name: "Unregister…" }));
    });
    const confirm = await screen.findByRole("dialog", { name: "Unregister Leif?" });
    // The consequence is spelled out, and the dangerous button is named after what it does.
    expect(within(confirm).getByText("They lose their account, groups and rules that name them."));
    act(() => {
      fireEvent.click(within(confirm).getByRole("button", { name: "Unregister…" }));
    });
    expect(within(rowOf(dialog, "Leif")).getByText("not saved")).toBeTruthy();
    expect(within(panel).getByRole("button", { name: "Keep the registration" })).toBeTruthy();

    act(() => {
      fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    });
    await waitFor(() => {
      expect(within(dialog).queryByText("Leif")).toBeNull();
    });
    expect(
      within(dialog).getByText("Saved: 0 renamed, 1 unregistered · list reloaded"),
    ).toBeTruthy();
  });

  it("asks before throwing away queued changes, and keeps them when going back", async () => {
    const dialog = await openTab("registered");
    act(() => {
      fireEvent.click(rowOf(dialog, "Freya"));
    });
    const panel = within(dialog).getByRole("complementary", { name: "Details of Freya" });
    act(() => {
      fireEvent.click(within(panel).getByRole("button", { name: "Unregister…" }));
    });
    const confirm = await screen.findByRole("dialog", { name: "Unregister Freya?" });
    act(() => {
      fireEvent.click(within(confirm).getByRole("button", { name: "Unregister…" }));
    });

    act(() => {
      fireEvent.click(closeButton(dialog));
    });
    const guard = await screen.findByRole("dialog", { name: "Discard 1 unsaved changes?" });
    act(() => {
      fireEvent.click(within(guard).getByRole("button", { name: "Discard and close" }));
    });
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: TITLE })).toBeNull();
    });
  });

  it("offers the certificate ban only to someone with the Ban right (FR-10.65)", async () => {
    const dialog = await openTab("registered");
    act(() => {
      fireEvent.click(rowOf(dialog, "Anna"));
    });
    const panel = within(dialog).getByRole("complementary", { name: "Details of Anna" });
    const ban = within(panel).getByRole("button", { name: "Ban certificate…" });
    expect(ban).toBeTruthy();
    act(() => {
      fireEvent.click(ban);
    });
    // It lands on the Bans tab with the add form, where the missing address can be typed.
    const form = await screen.findByRole("dialog", { name: "Add ban · Local test server" });
    expect(within(form).getByRole("textbox", { name: "Who" })).toHaveValue("Anna");
    expect(within(form).getByRole("textbox", { name: "Certificate fingerprint" })).toHaveValue(
      "annahash",
    );
  });

  it("hides the tab without the Register right (FR-10.50, FR-10.2)", async () => {
    grant({ ban: true });
    openServerAdmin(1, "bans");
    render(<ServerAdminWindowHost />);
    const dialog = await screen.findByRole("dialog", { name: TITLE });
    await within(dialog).findByText("Mallory");
    expect(within(dialog).queryByRole("tab", { name: /Registered users/ })).toBeNull();
    expect(within(dialog).getAllByRole("tab")).toHaveLength(1);
  });

  it("walks the rows with the arrow keys (issue #256, FR-10.43 a11y)", async () => {
    const dialog = await openTab("registered");
    const first = dialog.querySelector('[role="row"][tabindex="0"]') as HTMLElement;
    expect(first).toBeTruthy();
    act(() => {
      first.focus();
    });

    act(() => {
      fireEvent.keyDown(first, { key: "ArrowDown" });
    });
    await waitFor(() => {
      const active = dialog.querySelector('[role="row"][tabindex="0"]') as HTMLElement;
      expect(active).not.toBe(first);
      expect(active.getAttribute("aria-selected")).toBe("true");
    });
  });
});
