import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { MockApi } from "../../api";
import { axeViolations } from "../../test/axe";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { closePersonAction, openPersonAction, resetPersonAction } from "./personActionStore";
import { PersonActionHost } from "./PersonActionHost";

let api: MockApi;

/** Carla is on the mock server in Raid, and the mock gives her a documentation address. */
const CARLA = 4;
const SERVER = "Local test server";

function grant(): void {
  const root = getServerState().channels.get(0);
  if (!root) throw new Error("the mock has no root channel");
  api.mock.updateChannel(0, {
    ...root,
    permissions: { ...root.permissions, kick: true, ban: true },
  });
}

/** Opens the dialog for Carla and waits for the engine's answer. */
async function open(mode: "kick" | "ban"): Promise<HTMLElement> {
  openPersonAction(1, CARLA, "Carla", "Raid", mode);
  render(<PersonActionHost />);
  const title = mode === "kick" ? `Kick Carla? · ${SERVER}` : `Ban Carla · ${SERVER}`;
  return await screen.findByRole("dialog", { name: title });
}

beforeEach(async () => {
  api = await connectedMock({ bindStore: true });
  stubDialog();
  resetPersonAction();
  grant();
});

afterEach(() => {
  closePersonAction();
  resetPersonAction();
  resetAll();
});

describe("the kick dialog", () => {
  it("asks with the person line, keeps the reason optional and sends it (FR-10.11, FR-10.57)", async () => {
    const dialog = await open("kick");
    const lines = within(dialog);
    expect(lines.getByText("Carla · in Raid")).toBeTruthy();
    expect(lines.getByText("They can reconnect right away.")).toBeTruthy();
    expect(lines.getByText("shown to Carla")).toBeTruthy();
    expect(await axeViolations(dialog)).toEqual([]);

    const kick = lines.getByRole("button", { name: "Kick" });
    act(() => {
      fireEvent.click(kick);
    });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(getServerState().users.get(CARLA)).toBeUndefined();
  });

  it("keeps the dialog open and shows the server's refusal inside it (FR-10.59)", async () => {
    const root = getServerState().channels.get(0);
    if (!root) throw new Error("the mock has no root channel");
    api.mock.updateChannel(0, { ...root, permissions: { ...root.permissions, kick: false } });
    const dialog = await open("kick");
    act(() => {
      fireEvent.submit(within(dialog).getByText("shown to Carla").closest("form") as HTMLElement);
    });
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "The server refused: you do not have that right on the root channel.",
    );
    expect(getServerState().users.get(CARLA)).toBeDefined();
  });

  it("does not offer kicking yourself, and the engine refuses it (issue #253)", async () => {
    const self = getServerState().selfSession;
    if (self === null) throw new Error("the mock has no own session");
    await expect(api.personKick(1, self, null)).rejects.toMatchObject({ code: "invalidInput" });
  });
});

describe("the ban dialog", () => {
  it("adds one entry with the chosen length and the right end (FR-10.12)", async () => {
    const dialog = await open("ban");
    const lines = within(dialog);
    // The mock answers 7200 s, so the line says 2 h.
    expect(lines.getByText(/connected 2 h/)).toBeTruthy();
    expect(lines.getByText("Ends on", { exact: false })).toBeTruthy();
    expect(await axeViolations(dialog)).toEqual([]);

    act(() => {
      fireEvent.click(lines.getByRole("button", { name: "1 hour" }));
    });
    act(() => {
      fireEvent.click(lines.getByRole("button", { name: "Ban for 1 hour" }));
    });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    const entry = (await api.banListOpen(1)).find((e) => e.name === "Carla");
    expect(entry?.durationSecs).toBe(3600);
    expect(entry?.mask).toBe(32);
    expect(getServerState().users.get(CARLA)).toBeUndefined();
  });

  it("keeps a target the server cannot match off with its reason (FR-10.14)", async () => {
    api.mock.setPersonTarget(1, CARLA, { certHash: null });
    const dialog = await open("ban");
    const lines = within(dialog);
    const cert = lines.getByRole("switch", { name: "Certificate" });
    const ip = lines.getByRole("switch", { name: "IP address" });
    expect(cert).toHaveAttribute("aria-checked", "false");
    expect(cert).toBeDisabled();
    expect(
      lines.getByText("This person has no certificate, so only their address can be banned."),
    ).toBeTruthy();
    expect(ip).toHaveAttribute("aria-checked", "true");
    expect(await axeViolations(dialog)).toEqual([]);
  });

  it("fixes the length and both targets when the list cannot be read (FR-10.13)", async () => {
    api.mock.setPersonTarget(1, CARLA, { banListReadable: false });
    const dialog = await open("ban");
    const lines = within(dialog);
    expect(lines.getByText("This server only allows its default ban length")).toBeTruthy();
    expect(lines.getByRole("button", { name: "Server default" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(await axeViolations(dialog)).toEqual([]);
    act(() => {
      fireEvent.click(lines.getByRole("button", { name: "Ban" }));
    });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(getServerState().users.get(CARLA)).toBeUndefined();
  });

  it("warns for a wider range than the address itself (issue #253)", async () => {
    const dialog = await open("ban");
    const lines = within(dialog);
    act(() => {
      fireEvent.change(lines.getByRole("spinbutton", { name: "Subnet mask" }), {
        target: { value: "24" },
      });
    });
    expect(
      lines.getByText(/^A wider IP range \(for example \/24\) can lock out other people/),
    ).toBeTruthy();
    expect(await axeViolations(dialog)).toEqual([]);
  });

  it("asks about an entry the server changed before it overwrites it (FR-10.66)", async () => {
    // Someone else's change on the same address and certificate (FR-10.66).
    await api.banListSave(
      1,
      [
        {
          kind: "add",
          name: "Carla",
          hash: "carlahash",
          address: "198.51.100.4",
          mask: 32,
          reason: "someone else's reason",
          duration: 3600,
        },
      ],
      false,
    );
    const dialog = await open("ban");
    const lines = within(dialog);
    act(() => {
      fireEvent.change(lines.getByLabelText("Reason"), { target: { value: "spam links" } });
    });
    act(() => {
      fireEvent.click(lines.getByRole("button", { name: "Ban for 1 hour" }));
    });
    expect(
      await lines.findByText("Someone changed these bans while you were editing"),
    ).toBeTruthy();
    act(() => {
      fireEvent.click(lines.getByRole("button", { name: "Save mine anyway" }));
    });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    const list = await api.banListOpen(1);
    expect(list.filter((e) => e.name === "Carla")).toHaveLength(2);
    expect(list.some((e) => e.reason === "spam links")).toBe(true);
  });

  it("“Kick only instead” keeps what was typed and closes without banning (FR-10.12)", async () => {
    const dialog = await open("ban");
    const lines = within(dialog);
    act(() => {
      fireEvent.change(lines.getByLabelText("Reason"), { target: { value: "flooded the raid" } });
    });
    act(() => {
      fireEvent.click(lines.getByRole("button", { name: "Kick only instead" }));
    });
    const kick = await screen.findByRole("dialog", { name: `Kick Carla? · ${SERVER}` });
    expect(within(kick).getByLabelText("Reason")).toHaveValue("flooded the raid");
    expect(within(kick).getByText("They can reconnect right away.")).toBeTruthy();
  });
});
