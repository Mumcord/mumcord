import { act, fireEvent, render, screen, within } from "@testing-library/react";
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

let api: MumcordApi;

async function openWindow(): Promise<HTMLElement> {
  openServerAdmin(1);
  render(<ServerAdminWindowHost />);
  const dialog = await screen.findByRole("dialog", { name: "Ban list" });
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
  it("shows the entries with masked addresses and shortened fingerprints (FR-10.51)", async () => {
    const dialog = await openWindow();
    const rows = within(dialog);
    expect(rows.getByText("a3f9…6f70")).toBeTruthy();
    expect(rows.getByText("203.0.113.*/32")).toBeTruthy();
    expect(rows.getByText("spam links")).toBeTruthy();
    expect(rows.getByText("198.51.100.*/32")).toBeTruthy();
    expect(await axeViolations(dialog)).toEqual([]);
  });

  it("reveals one address and masks it again when hidden (FR-10.51)", async () => {
    const dialog = await openWindow();
    const row = rowOf(dialog, "Mallory");
    const show = within(row).getByRole("button", { name: "Show" });
    act(() => {
      fireEvent.click(show);
    });
    expect(within(row).getByText("203.0.113.1/32")).toBeTruthy();
    act(() => {
      fireEvent.click(within(row).getByRole("button", { name: "Hide" }));
    });
    expect(within(row).getByText("203.0.113.*/32")).toBeTruthy();
  });

  it("searches and filters while typing (FR-10.53)", async () => {
    const dialog = await openWindow();
    const rows = within(dialog);
    const search = rows.getByRole("searchbox");
    act(() => {
      fireEvent.change(search, { target: { value: "flooding" } });
    });
    expect(rows.queryByText("Mallory")).toBeNull();
    expect(rows.queryByText("(no name)")).toBeTruthy();

    act(() => {
      fireEvent.change(search, { target: { value: "" } });
    });
    act(() => {
      fireEvent.click(rows.getByRole("button", { name: "Expired" }));
    });
    expect(rows.getByText("No bans on this server")).toBeTruthy();
  });

  it("edits an entry in place and saves the changes together (FR-10.51, FR-10.66)", async () => {
    const dialog = await openWindow();
    const rows = within(dialog);
    const row = rowOf(dialog, "Mallory");
    act(() => {
      fireEvent.click(within(row).getByRole("button", { name: "Edit" }));
    });
    const panel = within(dialog).getByRole("group", { name: /Edit entry/ });
    const reason = within(panel).getByRole("textbox");
    act(() => {
      fireEvent.change(reason, { target: { value: "spam links" } });
      fireEvent.blur(reason);
    });
    act(() => {
      fireEvent.click(within(panel).getByRole("button", { name: "Permanent" }));
    });
    // Reason and end are one change for this entry.
    expect(rows.getByText("1")).toBeTruthy();

    const outcome = await saveChanges(false);
    expect(outcome?.kind).toBe("saved");
    expect(within(rowOf(dialog, "Mallory")).getByText("permanent")).toBeTruthy();
  });

  it("asks before unban, and the entry is removed on save (FR-10.55, FR-10.51)", async () => {
    const dialog = await openWindow();
    const rows = within(dialog);
    const row = rowOf(dialog, "Mallory");
    act(() => {
      fireEvent.click(within(row).getByRole("button", { name: "Edit" }));
    });
    const panel = within(dialog).getByRole("group", { name: /Edit entry/ });
    const confirmButton = within(panel).getByRole("button", { name: "Unban" });
    act(() => {
      fireEvent.click(confirmButton);
    });
    const confirm = await screen.findByRole("dialog", { name: /Unban Mallory/ });
    act(() => {
      fireEvent.click(within(confirm).getByRole("button", { name: "Unban" }));
    });
    expect(rows.getByText("1")).toBeTruthy();
    const saved = await saveChanges(false);
    expect(saved?.kind).toBe("saved");
    expect(rows.queryByText("Mallory")).toBeNull();
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
