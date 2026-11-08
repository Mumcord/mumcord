import { act, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import type { MockApi } from "../../api";
import { ServerInfoPanel } from "./ServerInfoPanel";

let api: MockApi;

/** Sets the root channel's permissions; the mock reports them as the server's own answer. */
function grant(patch: {
  write?: boolean;
  ban?: boolean;
  register?: boolean;
  kick?: boolean;
  muteDeafen?: boolean;
}): void {
  const root = getServerState().channels.get(0);
  if (!root) throw new Error("root channel missing");
  act(() => {
    api.mock.updateChannel(0, { ...root, permissions: { ...root.permissions, ...patch } });
  });
}

function renderPanel(): void {
  render(<ServerInfoPanel serverState={getServerState()} onClose={() => undefined} />);
}

beforeEach(async () => {
  api = await connectedMock({ bindStore: true });
  stubDialog();
});

afterEach(() => {
  resetAll();
});

describe("server info admin section", () => {
  it("shows no Admin section for a user without admin rights (FR-10.3, AC-10.1)", async () => {
    renderPanel();
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).queryByText("Your rights here")).toBeNull();
    expect(within(dialog).queryByText("Server notice")).toBeNull();
  });

  it("shows rights, limits, the welcome hint, the notice and the administration link (FR-10.54, AC-10.39)", async () => {
    grant({ write: true, ban: true, register: true, kick: true, muteDeafen: true });
    renderPanel();
    const dialog = await screen.findByRole("dialog");
    const section = within(dialog).getByText("Your rights here").closest("section");
    if (!section) throw new Error("admin section not found");
    expect(within(section).getByText("Server limits")).toBeTruthy();
    expect(within(section).getByText("Welcome text")).toBeTruthy();
    expect(within(section).getByText("Server notice")).toBeTruthy();
    expect(within(section).getByText("Ban list & registered users…")).toBeTruthy();
    expect(within(section).getByText("Edit…")).toBeTruthy();
    // Rights at the root in their plain names, joined on one line (Q3.11).
    expect(within(section).getByText(/Mute others · Edit permissions · Kick/)).toBeTruthy();
  });

  it("hides the notice Edit… without Write and the administration link without Ban or Register (FR-10.54)", async () => {
    grant({ kick: true });
    renderPanel();
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).queryByText("Edit…")).toBeNull();
    expect(within(dialog).queryByText("Ban list & registered users…")).toBeNull();
  });
});
