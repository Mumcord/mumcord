import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_CHANNEL_PASSWORD } from "../../api/mock";
import { PreferencesProvider } from "../../app/PreferencesProvider";
import type { MockApi, SavedChannelPassword, SavedServer } from "../../api";
import { i18n } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { ChannelsPanel } from "../channels/ChannelsPanel";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { resetFollowStore } from "../follow/followStore";
import { ChannelPasswordList } from "./ChannelPasswordList";
import { channelPasswordName } from "./channelPasswordName";
import { resetMentions } from "./mentions";
import { ServerDialog } from "./ServerDialog";
import { ServersRoot } from "./ServersRoot";

let api: MockApi;

async function renderClient() {
  api = await connectedMock();
  render(
    <>
      <nav aria-label="Servers">
        <ServersRoot />
      </nav>
      <main>
        <ChannelsPanel />
      </main>
    </>,
  );
  await screen.findByRole("tree", { name: "Channels" });
}

const raid = () => screen.getByRole("treeitem", { name: /^Raid/ });
const join = () => {
  fireEvent.keyDown(raid(), { key: "Enter", shiftKey: true });
};
const dialog = () => screen.findByRole("dialog", { name: "This channel needs a password" });
const submit = async (password: string) => {
  const view = await dialog();
  fireEvent.change(within(view).getByLabelText("Password"), { target: { value: password } });
  fireEvent.click(within(view).getByRole("button", { name: "Join" }));
};

beforeEach(stubDialog);
afterEach(() => {
  resetFollowStore();
  resetMentions();
  resetAll();
});

describe("channel passwords (FR-2.78)", () => {
  it("shows a lock on channels the user may not enter", async () => {
    await renderClient();
    expect(raid().querySelector(".tree-channel__lock")).toBeNull();
    api.mock.lockChannel(2);
    await waitFor(() => {
      expect(raid()).toHaveAccessibleName(/locked/);
    });
    expect(raid().querySelector(".tree-channel__lock")).not.toBeNull();
  });

  it("asks for a password when a join is refused and joins with the right one", async () => {
    await renderClient();
    api.mock.lockChannel(2);
    join();
    const view = await dialog();
    expect(view).toHaveTextContent("Enter the password for Raid.");
    expect(within(view).getByLabelText("Password")).toHaveFocus();
    expect(await axeViolations(view)).toEqual([]);

    const spy = vi.spyOn(api, "channelJoinWithPassword");
    await submit(MOCK_CHANNEL_PASSWORD);
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect(spy).toHaveBeenCalledWith(1, 2, MOCK_CHANNEL_PASSWORD);
    expect(getServerState().users.get(1)?.channelId).toBe(2);
    expect(raid().querySelector(".tree-channel__lock")).toBeNull();
  });

  it("says Wrong password and stays open for another try", async () => {
    await renderClient();
    api.mock.lockChannel(2);
    join();
    await submit("wrong");
    const view = await dialog();
    expect(await within(view).findByRole("alert")).toHaveTextContent("Wrong password");
    expect(within(view).getByLabelText("Password")).toHaveAttribute("aria-invalid", "true");
    expect(getServerState().users.get(1)?.channelId).not.toBe(2);
    expect(await api.channelPasswords(1)).toEqual([]);

    await submit(MOCK_CHANNEL_PASSWORD);
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
  });

  it("closes on Cancel without joining", async () => {
    await renderClient();
    api.mock.lockChannel(2);
    join();
    const view = await dialog();
    fireEvent.click(within(view).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(getServerState().users.get(1)?.channelId).not.toBe(2);
  });
});

describe("channelPasswordName (issue #146)", () => {
  const t = i18n.getFixedT("en", "servers");
  const saved = (label: string | null, ...paths: string[][]): SavedChannelPassword => ({
    label,
    channels: paths.map((path, channelId) => ({ channelId, path, found: true })),
    lastUsedAt: null,
  });

  it("names a channel with its parent", () => {
    expect(channelPasswordName(t, saved(null, ["Raid", "Officers"]), 0)).toBe("Officers (Raid)");
    expect(channelPasswordName(t, saved(null, ["Gaming Crew", "Raid", "Officers"]), 0)).toBe(
      "Officers (Raid)",
    );
    expect(channelPasswordName(t, saved(null, ["Raid"], ["AFK"]), 0)).toBe("Raid, AFK");
  });

  it("prefers the own label and falls back to Password N", () => {
    expect(channelPasswordName(t, saved("Mine", ["Raid"]), 0)).toBe("Mine");
    expect(channelPasswordName(t, saved(null), 2)).toBe("Password 3");
    expect(channelPasswordName(t, saved("Mine"), 0, " ")).toBe("Password 1");
  });
});

describe("stored channel passwords in the edit-server dialog (FR-2.78, issue #146)", () => {
  async function storeOne(): Promise<SavedServer> {
    api = await connectedMock();
    api.mock.lockChannel(2);
    await api.channelJoinWithPassword(1, 2, MOCK_CHANNEL_PASSWORD);
    const server = (await api.serversList()).find((s) => s.id === 1);
    if (!server) throw new Error("seed server missing");
    return server;
  }

  function renderDialog(server: SavedServer, onClose = vi.fn()) {
    render(
      <PreferencesProvider>
        <ServerDialog server={server} link={null} identityName="You" onClose={onClose} />
      </PreferencesProvider>,
    );
    // The saved channel passwords live in the Passwords tab (issue #151).
    fireEvent.click(screen.getByRole("tab", { name: "Passwords" }));
    return screen.findByRole("list", { name: "Channel passwords" });
  }

  const save = () => {
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
  };

  it("names a password after the channel it was entered for, masked, with its last use", async () => {
    const server = await storeOne();
    const list = await renderDialog(server);
    expect(list).toHaveTextContent("Raid ••••••••");
    expect(list).toHaveTextContent(/Last used/);
    expect(list).not.toHaveTextContent("(not found)");
    expect(list).not.toHaveTextContent(MOCK_CHANNEL_PASSWORD);
    expect(await axeViolations(list)).toEqual([]);
  });

  it("lists a password entered for two channels under both", async () => {
    const server = await storeOne();
    api.mock.lockChannel(3);
    await api.channelJoinWithPassword(1, 3, MOCK_CHANNEL_PASSWORD);
    const list = await renderDialog(server);
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    expect(list).toHaveTextContent("Raid, AFK ••••••••");
  });

  it("shows a channel that is gone as not found and keeps the name", async () => {
    const server = await storeOne();
    api.mock.renameChannel(2, "Renamed");
    const list = await renderDialog(server);
    expect(list).toHaveTextContent("Raid ••••••••");
    expect(list).toHaveTextContent("Raid (not found)");
  });

  it("keeps a password saved before as Password N", async () => {
    api = await connectedMock();
    api.mock.addBareChannelPassword(1, "old");
    const server = (await api.serversList()).find((s) => s.id === 1);
    if (!server) throw new Error("seed server missing");
    const list = await renderDialog(server);
    expect(within(list).getByText(/Password 1/)).toBeInTheDocument();
    expect(list).not.toHaveTextContent(/Last used/);
  });

  it("renames a password on save and goes back to the default when emptied", async () => {
    const server = await storeOne();
    const onClose = vi.fn();
    const list = await renderDialog(server, onClose);
    fireEvent.click(within(list).getByRole("button", { name: "Rename Raid" }));
    const field = within(list).getByLabelText("Name for Raid");
    expect(field).toHaveFocus();
    fireEvent.change(field, { target: { value: "Raid password" } });
    fireEvent.keyDown(field, { key: "Enter" });
    expect(onClose).not.toHaveBeenCalled();
    expect(within(list).getByText(/Raid password/)).toBeInTheDocument();

    const spy = vi.spyOn(api, "channelPasswordRename");
    save();
    await waitFor(() => {
      expect(onClose).toHaveBeenCalled();
    });
    expect(spy).toHaveBeenCalledWith(1, 0, "Raid password");
    expect((await api.channelPasswords(1))[0]?.label).toBe("Raid password");

    await api.channelPasswordRename(1, 0, " ");
    expect((await api.channelPasswords(1))[0]?.label).toBeNull();
  });

  it("removes one on save", async () => {
    const server = await storeOne();
    const onClose = vi.fn();
    const list = await renderDialog(server, onClose);
    const spy = vi.spyOn(api, "channelPasswordsRemove");
    fireEvent.click(within(list).getByRole("button", { name: "Remove Raid" }));
    expect(screen.queryByRole("list", { name: "Channel passwords" })).not.toBeInTheDocument();
    expect(await api.channelPasswords(1)).toHaveLength(1);

    save();
    await waitFor(() => {
      expect(onClose).toHaveBeenCalled();
    });
    expect(spy).toHaveBeenCalledWith(1, [0]);
    expect(await api.channelPasswords(1)).toEqual([]);
  });

  it("shows nothing for a server without channel passwords", async () => {
    api = await connectedMock();
    const { container } = render(
      <ChannelPasswordList
        serverId={1}
        removed={[]}
        onRemove={vi.fn()}
        renamed={new Map()}
        onRename={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(container).toBeEmptyDOMElement();
    });
  });
});
