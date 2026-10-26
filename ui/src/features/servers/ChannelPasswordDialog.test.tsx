import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_CHANNEL_PASSWORD } from "../../api/mock";
import { PreferencesProvider } from "../../app/PreferencesProvider";
import type { MockApi, SavedServer } from "../../api";
import { axeViolations } from "../../test/axe";
import { ChannelsPanel } from "../channels/ChannelsPanel";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { resetFollowStore } from "../follow/followStore";
import { ChannelPasswordList } from "./ChannelPasswordList";
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
    expect(await api.channelPasswordCount(1)).toBe(0);

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

describe("stored channel passwords in the edit-server dialog (FR-2.78)", () => {
  async function storeOne(): Promise<SavedServer> {
    api = await connectedMock();
    api.mock.lockChannel(2);
    await api.channelJoinWithPassword(1, 2, MOCK_CHANNEL_PASSWORD);
    const server = (await api.serversList()).find((s) => s.id === 1);
    if (!server) throw new Error("seed server missing");
    return server;
  }

  it("lists them masked and removes one on save", async () => {
    const server = await storeOne();
    const onClose = vi.fn();
    render(
      <PreferencesProvider>
        <ServerDialog server={server} link={null} identityName="You" onClose={onClose} />
      </PreferencesProvider>,
    );
    const list = await screen.findByRole("list", { name: "Channel passwords" });
    expect(within(list).getByText(/Channel password 1/)).toBeInTheDocument();
    expect(list).not.toHaveTextContent(MOCK_CHANNEL_PASSWORD);
    expect(await axeViolations(list)).toEqual([]);

    const spy = vi.spyOn(api, "channelPasswordsRemove");
    fireEvent.click(within(list).getByRole("button", { name: "Remove Channel password 1" }));
    expect(screen.queryByRole("list", { name: "Channel passwords" })).not.toBeInTheDocument();
    expect(await api.channelPasswordCount(1)).toBe(1);

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(onClose).toHaveBeenCalled();
    });
    expect(spy).toHaveBeenCalledWith(1, [0]);
    expect(await api.channelPasswordCount(1)).toBe(0);
  });

  it("shows nothing for a server without channel passwords", async () => {
    api = await connectedMock();
    const { container } = render(
      <ChannelPasswordList serverId={1} removed={[]} onRemove={vi.fn()} />,
    );
    await waitFor(() => {
      expect(container).toBeEmptyDOMElement();
    });
  });
});
