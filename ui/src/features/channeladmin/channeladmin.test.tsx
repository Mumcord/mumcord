import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ChannelPermissions, MockApi } from "../../api";
import { axeViolations } from "../../test/axe";
import { ChannelsPanel } from "../channels";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { ChannelTreeMenu } from "../channelheader";
import { resetFollowStore } from "../follow";
import { deleteImpact, descendants, parentOptions } from "./channelAdminModel";

let api: MockApi;

const NONE: ChannelPermissions = {
  enter: true,
  textMessage: true,
  listen: true,
  registerSelf: false,
  speak: true,
  whisper: true,
  write: false,
  moveUsers: false,
  ban: false,
  kick: false,
  muteDeafen: false,
  makeChannel: false,
  makeTempChannel: false,
  linkChannel: false,
  register: false,
  resetUserContent: false,
};
const ADMIN: ChannelPermissions = {
  ...NONE,
  write: true,
  makeChannel: true,
  makeTempChannel: true,
  linkChannel: true,
};

/** Seeded channels: Root 0 › Lobby 1, Raid 2, AFK 3; the own user is in Lobby. */
async function renderTree(rights: Record<number, ChannelPermissions> = {}) {
  resetFollowStore();
  api = await connectedMock({ bindStore: true });
  stubDialog();
  act(() => {
    for (const [id, permissions] of Object.entries(rights)) {
      api.mock.updateChannel(Number(id), { permissions });
    }
  });
  const view = render(
    <ChannelsPanel channelMenu={(menu) => <ChannelTreeMenu key={menu.channelId} {...menu} />} />,
  );
  await screen.findByRole("tree", { name: "Channels" });
  return view;
}

const row = (name: RegExp) => screen.getByRole("treeitem", { name });

function openAdmin(name: RegExp): HTMLElement {
  fireEvent.contextMenu(row(name), { clientX: 40, clientY: 50 });
  fireEvent.click(screen.getByRole("menuitem", { name: /^Admin/ }));
  return screen.getByRole("menu", { name: "Admin" });
}

function adminLabels(name: RegExp): string[] {
  return within(openAdmin(name))
    .getAllByRole("menuitem")
    .map((item) => item.textContent);
}

afterEach(() => {
  cleanup();
  resetAll();
});

describe("channel menu Admin ▸ (FR-10.8)", () => {
  it("lists every channel action in order for an admin, delete last", async () => {
    await renderTree({ 1: ADMIN, 2: { ...ADMIN } });
    act(() => {
      api.mock.updateChannel(2, { links: [3] });
    });
    expect(adminLabels(/^Raid/)).toEqual([
      "Create sub-channel",
      "Create temporary channel",
      "Edit channel",
      "Link with my channel",
      "Unlink all",
      "Delete channel",
    ]);
  });

  it("has no Admin entry without channel rights", async () => {
    await renderTree();
    fireEvent.contextMenu(row(/^Raid/), { clientX: 40, clientY: 50 });
    expect(screen.queryByRole("menuitem", { name: /^Admin/ })).not.toBeInTheDocument();
  });

  it("offers only the temporary channel when that is the only right", async () => {
    await renderTree({ 2: { ...NONE, makeTempChannel: true } });
    expect(adminLabels(/^Raid/)).toEqual(["Create temporary channel"]);
  });
});

describe("Channel window (FR-10.21–FR-10.24)", () => {
  it("creates a sub-channel with only the General tab", async () => {
    await renderTree({ 2: ADMIN });
    fireEvent.click(
      within(openAdmin(/^Raid/)).getByRole("menuitem", { name: "Create sub-channel" }),
    );
    const dialog = screen.getByRole("dialog", { name: "Create sub-channel" });
    expect(within(dialog).queryByRole("tablist")).not.toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Create" })).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("Name"), { target: { value: "Raid night" } });
    fireEvent.change(within(dialog).getByLabelText("Maximum users"), { target: { value: "5" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Create" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect(await screen.findByRole("treeitem", { name: /^Raid night/ })).toBeInTheDocument();
  });

  it("shows the server's refusal and keeps the window open", async () => {
    await renderTree({ 0: ADMIN });
    fireEvent.click(
      within(openAdmin(/^Root/)).getByRole("menuitem", { name: "Create sub-channel" }),
    );
    const dialog = screen.getByRole("dialog", { name: "Create sub-channel" });
    // Standard servers refuse a name a sibling already has.
    fireEvent.change(within(dialog).getByLabelText("Name"), { target: { value: "Raid" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Create" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "This channel name is not allowed.",
    );
  });

  it("saves only what changed and asks before moving under another parent", async () => {
    await renderTree({ 2: ADMIN });
    fireEvent.keyDown(row(/^Raid/), { key: "F2" });
    const dialog = screen.getByRole("dialog", { name: "Edit channel" });
    expect(within(dialog).getByRole("tab", { name: "General" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    const save = within(dialog).getByRole("button", { name: "Save changes" });
    expect(save).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("Name"), { target: { value: "Raid 2" } });
    const parent = within(dialog).getByLabelText("Parent channel");
    fireEvent.change(parent, { target: { value: "AFK" } });
    fireEvent.keyDown(parent, { key: "Enter" });
    expect(await axeViolations(dialog)).toEqual([]);
    fireEvent.click(save);
    const confirm = screen.getByRole("alertdialog", { name: "Move Raid 2 under AFK?" });
    expect(confirm).toHaveTextContent("It has no sub-channels.");
    fireEvent.click(within(confirm).getByRole("button", { name: "Move" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    fireEvent.keyDown(row(/^AFK/), { key: "ArrowRight" });
    expect(await screen.findByRole("treeitem", { name: /^Raid 2/ })).toBeInTheDocument();
  });

  it("links channels on the Links tab and greys out those it may not link", async () => {
    await renderTree({ 2: ADMIN, 1: ADMIN });
    fireEvent.click(within(openAdmin(/^Raid/)).getByRole("menuitem", { name: "Edit channel" }));
    const dialog = screen.getByRole("dialog", { name: "Edit channel" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Links" }));
    const afk = within(dialog).getByRole("checkbox", { name: /AFK/ });
    expect(afk).toBeDisabled();
    expect(afk).toHaveAccessibleDescription("You can't link this one");
    fireEvent.click(within(dialog).getByRole("checkbox", { name: /Lobby/ }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Save changes" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect(adminLabels(/^Raid/)).toContain("Unlink all");
  });
});

describe("Password tab (FR-10.28, #251)", () => {
  async function openPassword() {
    fireEvent.keyDown(row(/^Raid/), { key: "F2" });
    const dialog = screen.getByRole("dialog", { name: "Edit channel" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Password" }));
    return {
      dialog,
      field: await within(dialog).findByLabelText("Password", { selector: "input" }),
    };
  }

  it("sets a password with the official client's two rules and locks the channel", async () => {
    await renderTree({ 2: ADMIN });
    const { dialog, field } = await openPassword();
    expect(field).toHaveValue("");
    fireEvent.change(field, { target: { value: "secret" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save changes" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    const serverId = 1;
    const acl = await api.channelAclRead(serverId, 2);
    expect(acl.password).toEqual({ kind: "set", password: "secret" });
    expect(acl.rules.filter((r) => !r.inherited).map((r) => r.group)).toEqual(["all", "#secret"]);
  });

  it("asks when someone changed the rules meanwhile", async () => {
    await renderTree({ 2: ADMIN });
    const { dialog, field } = await openPassword();
    const serverId = 1;
    // Another admin saves a password while this window is open.
    const before = await api.channelAclRead(serverId, 2);
    await act(() => api.channelPasswordSave(serverId, 2, "theirs", before, false));
    fireEvent.change(field, { target: { value: "mine" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save changes" }));
    const ask = await screen.findByRole("alertdialog", {
      name: "Someone changed these permissions while you were editing",
    });
    fireEvent.click(within(ask).getByRole("button", { name: "Save mine anyway" }));
    await waitFor(async () => {
      expect((await api.channelAclRead(serverId, 2)).password).toEqual({
        kind: "set",
        password: "mine",
      });
    });
  });
});

describe("Delete channel (FR-10.25, FR-10.10)", () => {
  it("opens from the Delete key with counts and deletes", async () => {
    await renderTree({ 1: ADMIN });
    fireEvent.keyDown(row(/^Lobby/), { key: "Delete" });
    const confirm = screen.getByRole("alertdialog", { name: "Delete channel Lobby?" });
    expect(confirm).toHaveTextContent(/\d+ (person is|people are) moved to Root\./);
    fireEvent.click(within(confirm).getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(screen.queryByRole("treeitem", { name: /^Lobby/ })).not.toBeInTheDocument();
    });
  });

  it("does nothing on F2 or Delete without the right", async () => {
    await renderTree();
    fireEvent.keyDown(row(/^Raid/), { key: "F2" });
    fireEvent.keyDown(row(/^Raid/), { key: "Delete" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });
});

describe("channel admin model", () => {
  const channel = (id: number, parentId: number | null, temporary = false) => ({
    id,
    parentId,
    name: `c${String(id)}`,
    position: 0,
    permissions: NONE,
    description: null,
    hasDescription: false,
    maxUsers: null,
    temporary,
    links: [],
    enterRestricted: false,
  });
  const channels = new Map(
    [channel(0, null), channel(1, 0), channel(2, 1), channel(3, 2), channel(4, 0, true)].map(
      (c) => [c.id, c],
    ),
  );

  it("finds sub-channels at every depth and never offers a loop as parent", () => {
    expect([...descendants(channels, 1)].sort()).toEqual([2, 3]);
    expect(parentOptions(channels, 1).map((c) => c.id)).toEqual([0]);
    expect(parentOptions(channels, null).map((c) => c.id)).toEqual([0, 1, 2, 3]);
  });

  it("counts the people a delete moves", () => {
    const users = new Map([
      [1, { session: 1, channelId: 3 }],
      [2, { session: 2, channelId: 0 }],
    ]) as unknown as Parameters<typeof deleteImpact>[1];
    expect(deleteImpact(channels, users, 1)).toEqual({ subChannels: 2, people: 1 });
  });
});
