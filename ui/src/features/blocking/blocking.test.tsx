import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BlockedUser, ChatMessage, MockApi, Settings } from "../../api";
import { axeViolations } from "../../test/axe";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import type { ConversationLog } from "../chat/chatStore";
import { blockedView } from "../chat/blockedView";
import { MessageList } from "../chat/MessageList";
import { MembersPanel } from "../members/MembersPanel";
import { PersonMenuHost } from "../members/PersonMenu";
import { closePersonMenu } from "../members/personMenuStore";
import { BlockedMessagesGroup, BlockedUsersGroup } from "../settings/sections/BlockedUsersGroup";
import { BlockDialogHost } from "./BlockDialog";
import { closeBlockDialog, openBlockDialog } from "./blockDialogStore";
import { findBlock, isBlocked, resetBlockedStore, useBlockedMessages } from "./blockedStore";

let api: MockApi;

function block(overrides: Partial<BlockedUser>): BlockedUser {
  return {
    id: 1,
    ownCertHash: "me",
    certHash: "annahash",
    name: "Anna",
    serverId: 1,
    serverLabel: "Gaming Crew",
    blockedAt: "2026-10-12T10:00:00Z",
    ...overrides,
  };
}

function message(id: string, overrides: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id,
    conversation: { kind: "channel", channelId: 1 },
    senderName: "Anna",
    senderCertHash: "annahash",
    senderSession: 2,
    body: `text ${id}`,
    sentAt: "2026-10-31T10:00:00Z",
    own: false,
    dismissedPreviews: [],
    syncedFrom: null,
    ...overrides,
  };
}

function log(messages: ChatMessage[]): ConversationLog {
  return {
    messages,
    loaded: true,
    loading: false,
    hasMore: false,
    loadFailed: false,
    syncing: false,
    syncResult: null,
    syncFailed: false,
  };
}

beforeEach(stubDialog);
afterEach(() => {
  act(closePersonMenu);
  act(closeBlockDialog);
  resetBlockedStore();
  resetAll();
});

describe("findBlock", () => {
  const list = [
    block({ id: 1 }),
    block({ id: 2, certHash: null, name: "Ben", serverId: 3 }),
    block({ id: 3, ownCertHash: "other", certHash: "carlahash", name: "Carla" }),
  ];

  it("matches a certificate block on every server, whatever the name", () => {
    expect(findBlock(list, "me", 1, "annahash", "Anna")?.id).toBe(1);
    expect(findBlock(list, "me", 9, "annahash", "NewName")?.id).toBe(1);
  });

  it("matches a name block only on its server, ignoring case", () => {
    expect(findBlock(list, "me", 3, null, "BEN")?.id).toBe(2);
    expect(findBlock(list, "me", 1, null, "Ben")).toBeUndefined();
  });

  it("only counts blocks of the identity in use", () => {
    expect(findBlock(list, "me", 1, "carlahash", "Carla")).toBeUndefined();
    expect(findBlock(list, "other", 1, "carlahash", "Carla")?.id).toBe(3);
    // Identities not known yet: every block counts.
    expect(findBlock(list, null, 1, "carlahash", "Carla")?.id).toBe(3);
  });
});

describe("blocked users store", () => {
  it("follows blockedUsersChanged and answers isBlocked", async () => {
    api = await connectedMock();
    expect(isBlocked(1, "annahash", "Anna")).toBe(false);
    await act(async () => {
      await api.blockUser(1, 2);
    });
    await waitFor(() => {
      expect(isBlocked(1, "annahash", "Anna")).toBe(true);
    });
    expect(isBlocked(1, "mayahash", "Maya")).toBe(false);
  });

  it("reads the blocked messages setting", async () => {
    api = await connectedMock();
    const settings = await api.settingsGet();
    await api.settingsUpdate({ ...settings, blockedMessages: "hide" } satisfies Settings);
    resetBlockedStore();
    function Mode() {
      return <span data-testid="mode">{useBlockedMessages()}</span>;
    }
    render(<Mode />);
    await waitFor(() => {
      expect(screen.getByTestId("mode")).toHaveTextContent("hide");
    });
  });
});

describe("person menu and dialog", () => {
  async function renderPanel() {
    api = await connectedMock();
    render(
      <>
        <MembersPanel />
        <PersonMenuHost />
        <BlockDialogHost />
      </>,
    );
    await screen.findByRole("heading", { name: "In Lobby (4)" });
  }
  const member = (name: RegExp) => screen.getByRole("button", { name });

  it("offers Block… last, confirms, marks the person and then offers Unblock", async () => {
    await renderPanel();
    const blockUser = vi.spyOn(api, "blockUser");
    fireEvent.contextMenu(member(/^Anna/), { clientX: 1, clientY: 1 });
    const items = screen.getAllByRole("menuitem");
    expect(items.at(-1)).toHaveTextContent("Block…");
    fireEvent.click(screen.getByRole("menuitem", { name: "Block…" }));

    const dialog = screen.getByRole("dialog", { name: "Block Anna?" });
    expect(dialog).toHaveTextContent("on Local test server");
    expect(dialog).toHaveTextContent("Private messages from Anna are not shown");
    expect(dialog).toHaveTextContent("Anna's channel messages are collapsed");
    expect(dialog).toHaveTextContent("follows Anna's certificate");
    expect(dialog).toHaveAccessibleDescription(/Anna can still see you/);
    expect(within(dialog).queryByRole("checkbox")).toBeNull();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toHaveFocus();
    expect(await axeViolations(dialog)).toEqual([]);

    fireEvent.click(within(dialog).getByRole("button", { name: "Block Anna" }));
    expect(blockUser).toHaveBeenCalledWith(1, 2);
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => {
      expect(member(/^Anna/)).toHaveAccessibleName(/Blocked/);
    });

    const unblock = vi.spyOn(api, "unblock");
    fireEvent.contextMenu(member(/^Anna/), { clientX: 1, clientY: 1 });
    fireEvent.click(screen.getByRole("menuitem", { name: "Unblock" }));
    expect(unblock).toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => {
      expect(member(/^Anna/)).not.toHaveAccessibleName(/Blocked/);
    });
  });

  it("does not offer Block for yourself", async () => {
    await renderPanel();
    fireEvent.contextMenu(member(/^Zoe/), { clientX: 1, clientY: 1 });
    expect(screen.queryByRole("menuitem", { name: "Block…" })).toBeNull();
  });

  it("says a block without certificate covers this name on this server", async () => {
    api = await connectedMock();
    render(<BlockDialogHost />);
    act(() => {
      openBlockDialog({
        serverId: 1,
        name: "Ben",
        hasCert: false,
        how: { kind: "session", session: 3 },
        opener: null,
      });
    });
    await waitFor(() => {
      expect(screen.getByRole("dialog")).toHaveTextContent(
        "Ben has no certificate, so the block applies to this name on Local test server only.",
      );
    });
  });

  it("blocks someone offline by certificate", async () => {
    api = await connectedMock();
    const blockCert = vi.spyOn(api, "blockCert");
    render(<BlockDialogHost />);
    act(() => {
      openBlockDialog({
        serverId: 1,
        name: "Noah",
        hasCert: true,
        how: { kind: "cert", certHash: "noahhash" },
        opener: null,
      });
    });
    fireEvent.click(screen.getByRole("button", { name: "Block Noah" }));
    expect(blockCert).toHaveBeenCalledWith(1, "noahhash", "Noah");
  });
});

describe("blocked messages in chat", () => {
  const blockedAnna = (m: ChatMessage) => m.senderCertHash === "annahash";
  const lobby = { kind: "channel", channelId: 1 } as const;

  it("collapses or hides channel messages, per setting", () => {
    const input = log([message("a"), message("b", { senderCertHash: "mayahash" })]);
    const collapsed = blockedView(input, lobby, blockedAnna, "collapse");
    expect(collapsed.log).toBe(input);
    expect([...(collapsed.collapsedIds ?? [])]).toEqual(["a"]);
    const hidden = blockedView(input, lobby, blockedAnna, "hide");
    expect(hidden.log.messages.map((m) => m.id)).toEqual(["b"]);
    expect(hidden.collapsedIds).toBeUndefined();
  });

  it("always hides private messages and never touches own messages", () => {
    const conversation = { kind: "private", certHash: "annahash" } as const;
    const input = log([message("a", { conversation }), message("o", { own: true, conversation })]);
    const view = blockedView(input, conversation, () => true, "collapse");
    expect(view.log.messages.map((m) => m.id)).toEqual(["o"]);
  });

  it("draws a collapsed message with Show, which reveals that one message", async () => {
    api = await connectedMock();
    render(
      <MessageList
        log={log([message("a"), message("b")])}
        label="Messages"
        onLoadOlder={vi.fn()}
        collapsedIds={new Set(["a", "b"])}
      />,
    );
    expect(screen.getAllByTestId("blocked-message")).toHaveLength(2);
    expect(screen.queryByText("text a")).toBeNull();
    const [firstShow] = screen.getAllByRole("button", {
      name: "Show message from a blocked user",
    });
    if (!firstShow) throw new Error("no Show button");
    fireEvent.click(firstShow);
    expect(screen.getByText("text a")).toBeInTheDocument();
    expect(screen.getAllByTestId("blocked-message")).toHaveLength(1);
  });
});

describe("settings → privacy", () => {
  it("lists blocks with server and date, and unblocks", async () => {
    api = await connectedMock();
    render(<BlockedUsersGroup />);
    expect(await screen.findByTestId("blocked-empty")).toHaveTextContent(
      "You haven't blocked anyone.",
    );
    await act(async () => {
      await api.blockUser(1, 2);
    });
    const table = await screen.findByRole("table");
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((h) => h.textContent),
    ).toEqual(["User", "Server", "Blocked since", "Actions"]);
    const row = within(table).getAllByRole("row")[1];
    if (!row) throw new Error("no blocked user row");
    expect(row).toHaveTextContent("Anna");
    expect(row).toHaveTextContent("Local test server");
    expect(await axeViolations(table)).toEqual([]);
    fireEvent.click(within(row).getByRole("button", { name: "Unblock Anna" }));
    expect(await screen.findByTestId("blocked-empty")).toBeInTheDocument();
  });

  it("switches between collapse and hide", async () => {
    api = await connectedMock();
    const settings = await api.settingsGet();
    const update = vi.fn();
    render(<BlockedMessagesGroup settings={settings} update={update} />);
    expect(screen.getByRole("radio", { name: "Collapse them" })).toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: "Hide them completely" }));
    expect(update).toHaveBeenCalledWith({ blockedMessages: "hide" });
  });
});
