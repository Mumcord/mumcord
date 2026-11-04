import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { setApi, type MumcordApi } from "../../api";
import { axeViolations } from "../../test/axe";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import {
  closeServerAdmin,
  openServerAdmin,
  resetServerAdmin,
  saveChanges,
  stageChange,
} from "./banStore";
import { ServerAdminWindowHost } from "./ServerAdminWindow";

const MALLORY = "203.0.113.1/32|a3f9c21e8b4d6f2a1c5e7b9d0f1a2b3c4d5e6f70";
const TITLE = "Server administration · Local test server";

let api: MumcordApi;

async function openWindow(): Promise<HTMLElement> {
  openServerAdmin(1);
  render(<ServerAdminWindowHost />);
  const dialog = await screen.findByRole("dialog", { name: TITLE });
  await within(dialog).findByText("Mallory");
  return dialog;
}

/** The table row the test acts on. */
function rowOf(dialog: HTMLElement, name: string): HTMLElement {
  return within(dialog).getByText(name).closest('[role="row"]') as HTMLElement;
}

beforeEach(async () => {
  api = await connectedMock({ bindStore: true });
  stubDialog();
  resetServerAdmin();
});

afterEach(() => {
  closeServerAdmin();
  resetServerAdmin();
  resetAll();
});

describe("ban list window", () => {
  it("names the window and its tab, and shows the entries masked and shortened (FR-10.50, FR-10.51)", async () => {
    const dialog = await openWindow();
    const rows = within(dialog);
    expect(rows.getByText("Bans (6)")).toBeTruthy();
    expect(within(rowOf(dialog, "Mallory")).getByText("a3f9…6f70")).toBeTruthy();
    expect(within(rowOf(dialog, "Mallory")).getByText("203.0.113.*/32")).toBeTruthy();
    expect(rows.getAllByText("spam links")).toHaveLength(2);
    expect(rows.getAllByText("(no name)")).toHaveLength(2);
    expect(await axeViolations(dialog)).toEqual([]);
  });

  it("reveals one address and masks it again when hidden (FR-10.51)", async () => {
    const dialog = await openWindow();
    const row = rowOf(dialog, "Mallory");
    const show = within(row).getByRole("button", { name: "Show the full IP address of Mallory" });
    act(() => {
      fireEvent.click(show);
    });
    expect(within(row).getByText("203.0.113.1/32")).toBeTruthy();
    act(() => {
      fireEvent.click(within(row).getByRole("button", { name: "Hide the IP address of Mallory" }));
    });
    expect(within(row).getByText("203.0.113.*/32")).toBeTruthy();
  });

  it("says what search and filters did, with a count per filter (issue #436)", async () => {
    const dialog = await openWindow();
    const rows = within(dialog);
    expect(rows.getByText("6 of 6 entries")).toBeTruthy();
    expect(rows.getByRole("button", { name: "All (6)" })).toBeTruthy();
    expect(rows.getByRole("button", { name: "Active (4)" })).toBeTruthy();
    expect(rows.getByRole("button", { name: "Expired (2)" })).toBeTruthy();

    act(() => {
      fireEvent.change(rows.getByRole("searchbox"), { target: { value: "flooding" } });
    });
    expect(rows.getByText("2 of 6 entries")).toBeTruthy();

    act(() => {
      fireEvent.change(rows.getByRole("searchbox"), { target: { value: "nobody" } });
    });
    expect(rows.getByText("No match for this search")).toBeTruthy();

    act(() => {
      fireEvent.change(rows.getByRole("searchbox"), { target: { value: "" } });
    });
    act(() => {
      fireEvent.click(rows.getByRole("button", { name: "Expired (2)" }));
    });
    expect(rows.getByText("Priya")).toBeTruthy();
    expect(rows.getByText("Tim")).toBeTruthy();
    expect(rows.queryByText("Mallory")).toBeNull();
  });

  it("marks expired rows with a word, not only a colour (issue #436)", async () => {
    const dialog = await openWindow();
    const row = rowOf(dialog, "Priya");
    expect(row).toHaveAttribute("data-expired");
    expect(within(row).getByText("Expired")).toBeTruthy();
  });

  it("changes an entry through the same form as adding, and saves (FR-10.51, FR-10.66)", async () => {
    const dialog = await openWindow();
    const rows = within(dialog);
    act(() => {
      fireEvent.click(rowOf(dialog, "Mallory"));
    });
    act(() => {
      fireEvent.click(rows.getByRole("button", { name: "Edit…" }));
    });
    const form = await screen.findByRole("dialog", {
      name: "Edit ban · Local test server",
    });
    // Both forms offer the same durations (FR-10.12).
    for (const chip of ["1 hour", "1 day", "7 days", "30 days", "Permanent", "Custom date…"]) {
      expect(within(form).getByRole("button", { name: chip })).toBeTruthy();
    }
    const reason = within(form).getByRole("textbox");
    act(() => {
      fireEvent.change(reason, { target: { value: "spam links and bots" } });
    });
    act(() => {
      fireEvent.click(within(form).getByRole("button", { name: "Change it" }));
    });
    expect(rows.getByText("not saved")).toBeTruthy();
    expect(rows.getByText("1", { selector: ".admin-badge" })).toBeTruthy();

    await act(async () => {
      await saveChanges(false);
    });
    expect(within(rowOf(dialog, "Mallory")).getByText("spam links and bots")).toBeTruthy();
  });

  it("adds an entry from the footer with the same form (issue #436)", async () => {
    const dialog = await openWindow();
    const rows = within(dialog);
    act(() => {
      fireEvent.click(rows.getByRole("button", { name: "+ Add ban…" }));
    });
    const form = await screen.findByRole("dialog", { name: "Add ban · Local test server" });
    const boxes = within(form).getAllByRole("textbox");
    act(() => {
      fireEvent.change(boxes[0] as HTMLElement, { target: { value: "Nora" } });
      fireEvent.change(boxes[2] as HTMLElement, { target: { value: "192.0.2.77" } });
      fireEvent.change(boxes[3] as HTMLElement, { target: { value: "flooding" } });
    });
    act(() => {
      fireEvent.click(within(form).getByRole("button", { name: "Add to the list" }));
    });
    expect(rows.getByText("Nora")).toBeTruthy();
    expect(within(rowOf(dialog, "Nora")).getByText("192.0.2.*/32")).toBeTruthy();
    expect(rows.getByRole("button", { name: "All (7)" })).toBeTruthy();
  });

  it("names the reason and the end in the unban question (FR-10.51)", async () => {
    const dialog = await openWindow();
    const rows = within(dialog);
    act(() => {
      fireEvent.click(rowOf(dialog, "Mallory"));
    });
    act(() => {
      fireEvent.click(rows.getByRole("button", { name: "Remove…" }));
    });
    const confirm = await screen.findByRole("dialog", { name: "Unban Mallory?" });
    expect(within(confirm).getByText(/Reason: spam links · until/)).toBeTruthy();
    act(() => {
      fireEvent.click(within(confirm).getByRole("button", { name: "Unban" }));
    });
    expect(rows.getByText("not saved")).toBeTruthy();
  });

  it("asks before it throws unsaved changes away (issue #436)", async () => {
    const dialog = await openWindow();
    act(() => {
      stageChange({ kind: "edit", key: MALLORY, reason: "spam links", duration: 0 });
    });
    expect(within(dialog).getByText("not saved")).toBeTruthy();

    act(() => {
      dialog.dispatchEvent(new Event("cancel"));
    });
    const asked = await screen.findByRole("dialog", { name: "Discard 1 unsaved changes?" });
    act(() => {
      fireEvent.click(within(asked).getByRole("button", { name: "Cancel" }));
    });
    expect(screen.getByRole("dialog", { name: TITLE })).toBeTruthy();

    act(() => {
      dialog.dispatchEvent(new Event("cancel"));
    });
    const again = await screen.findByRole("dialog", { name: "Discard 1 unsaved changes?" });
    act(() => {
      fireEvent.click(within(again).getByRole("button", { name: "Discard and close" }));
    });
    expect(screen.queryByRole("dialog", { name: TITLE })).toBeNull();
  });

  it("moves through the rows with the arrow keys and labels each button with its row", async () => {
    const dialog = await openWindow();
    const rows = within(dialog);
    expect(rows.getByRole("button", { name: "Copy the full fingerprint of Mallory" })).toBeTruthy();
    expect(rows.getByRole("button", { name: "Copy the full IP address of Mallory" })).toBeTruthy();
    const first = within(dialog).getAllByRole("row")[1] as HTMLElement;
    const second = within(dialog).getAllByRole("row")[2] as HTMLElement;
    act(() => {
      first.focus();
    });
    act(() => {
      fireEvent.keyDown(first, { key: "ArrowDown" });
    });
    await waitFor(() => {
      expect(second).toHaveAttribute("aria-selected", "true");
    });
    await waitFor(() => {
      expect(second).toHaveFocus();
    });
  });

  it("says what the save did and that the list came from the server (FR-10.54)", async () => {
    const dialog = await openWindow();
    act(() => {
      stageChange({ kind: "edit", key: MALLORY, reason: "spam links", duration: 0 });
      stageChange({ kind: "remove", key: "198.51.100.9/32|" });
    });
    const save = within(dialog).getByRole("button", { name: "Save ban list" });
    act(() => {
      fireEvent.click(save);
    });
    expect(
      await within(dialog).findByText("Saved: 0 added, 1 edited, 1 removed · list reloaded"),
    ).toBeTruthy();
  });

  it("turns read-only when the connection ends and offers only Close (FR-10.58)", async () => {
    const dialog = await openWindow();
    act(() => {
      stageChange({ kind: "edit", key: MALLORY, reason: "spam links", duration: 0 });
    });
    await act(async () => {
      await api.disconnect(1);
    });
    await waitFor(() => {
      expect(within(dialog).getByText(/Disconnected from Local test server/)).toBeTruthy();
    });
    expect(within(dialog).getByText(/Unsaved changes are gone\./)).toBeTruthy();
    expect(within(dialog).queryByRole("button", { name: "Save ban list" })).toBeNull();
    expect(within(dialog).getByRole("button", { name: "Close" })).toBeTruthy();
  });

  it("shows skeletons while the server's list is on its way (issue #436)", async () => {
    setApi({ ...api, banListOpen: () => new Promise(() => undefined) });
    openServerAdmin(1);
    render(<ServerAdminWindowHost />);
    const dialog = await screen.findByRole("dialog", { name: TITLE });
    expect(within(dialog).getByRole("table").getAttribute("aria-busy")).toBe("true");
    expect(within(dialog).getByText("Loading…")).toBeTruthy();
  });

  it("asks when the server changed the entry meanwhile (FR-10.66)", async () => {
    const dialog = await openWindow();
    const rows = within(dialog);
    setApi({
      ...api,
      banListSave: () =>
        Promise.resolve({
          kind: "conflict",
          key: MALLORY,
          fresh: [
            {
              key: MALLORY,
              name: "Mallory",
              certHash: null,
              address: "203.0.113.1",
              mask: 32,
              reason: "other reason",
              start: null,
              durationSecs: 0,
              expired: false,
            },
          ],
        }),
    });
    stageChange({ kind: "edit", key: MALLORY, reason: "spam links", duration: 0 });
    await saveChanges(false);
    const asked = await screen.findByRole("dialog", { name: /Someone changed/ });
    act(() => {
      fireEvent.click(within(asked).getByRole("button", { name: "Back" }));
    });
    expect(rows.getByText("Changes not saved yet – the whole list is sent on save")).toBeTruthy();
  });
});
