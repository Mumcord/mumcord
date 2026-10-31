import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MockApi, Settings } from "../../api";
import { chooseOption } from "../../components/SelectTestUtils";
import { getServerState, shownServerId } from "../channels/serverStore";
import { connectedMock, resetAll } from "../channels/testUtils";
import { resetFriendsStore } from "../friends";
import { PersonMenuHost } from "../members/PersonMenu";
import { closePersonMenu, openPersonMenu } from "../members/personMenuStore";
import { AcceptedConversationsGroup } from "../settings/sections/AcceptedConversationsGroup";
import { PrivateMessagesGroup } from "../settings/sections/PrivateMessagesGroup";
import { ChatRequestDialogHost } from "./ChatRequestDialog";
import { closeChatRequestDialog } from "./chatRequestDialogStore";
import { RequestsEntry } from "./RequestsEntry";
import { RequestsView } from "./RequestsView";
import { resetRequestsStore, usePmRequests } from "./requestsStore";
import { resetRequestsView, useRequestsView } from "./requestsView";

let api: MockApi;

async function connected() {
  api = await connectedMock({ bindStore: true });
  await waitFor(() => {
    expect(getServerState().users.has(2)).toBe(true);
  });
}

/** Anna (session 2) as a Mumcord user who can receive chat requests. */
function annaAcceptsChatRequests() {
  const anna = getServerState().users.get(2);
  if (!anna) throw new Error("Anna is missing");
  act(() => {
    api.mock.upsertUser({ ...anna, isMumcord: true, mumcordFeatures: ["chat-request"] });
  });
}

afterEach(() => {
  act(closePersonMenu);
  act(closeChatRequestDialog);
  resetRequestsView();
  resetRequestsStore();
  resetFriendsStore();
  resetAll();
});

function IncomingCount({ serverId }: { serverId: number }) {
  return <output>{usePmRequests(serverId).incoming.length}</output>;
}

/** The entry plus the view, wired like the channel column and the chat mount of the app. */
function RequestsHarness() {
  const view = useRequestsView();
  return (
    <>
      <RequestsEntry />
      {view && <RequestsView serverId={view.serverId} selectedId={view.selectedId} />}
    </>
  );
}

describe("requests store", () => {
  it("loads on sync, follows pmRequestsChanged and drops the requests on disconnect", async () => {
    await connected();
    const serverId = shownServerId();
    api.mock.receivePmRequest(2, { chatRequest: true, note: "hi" });
    render(<IncomingCount serverId={serverId} />);
    expect(await screen.findByText("1")).toBeInTheDocument();
    act(() => {
      api.mock.emit("pmRequestsChanged", { serverId, requests: { incoming: [], sent: [] } });
    });
    expect(screen.getByText("0")).toBeInTheDocument();
    act(() => {
      api.mock.receivePmRequest(2, { chatRequest: true, note: null });
    });
    // The emitted empty list was only an event; the engine still holds the first request.
    expect(screen.getByText("2")).toBeInTheDocument();
    act(() => {
      api.mock.emit("connectionStateChanged", {
        serverId,
        state: { state: "disconnected", error: null },
      });
    });
    expect(screen.getByText("0")).toBeInTheDocument();
  });

  it("lists accepted conversations and revokes one", async () => {
    await connected();
    annaAcceptsChatRequests();
    await act(async () => {
      await api.chatRequestSend(shownServerId(), 2, null);
    });
    const revokeSpy = vi.spyOn(api, "acceptedConversationRevoke");
    render(<AcceptedConversationsGroup />);
    const revoke = await screen.findByRole("button", {
      name: "Revoke the accepted conversation with Anna",
    });
    fireEvent.click(revoke);
    expect(revokeSpy).toHaveBeenCalledWith(expect.any(String), "annahash");
    expect(await screen.findByText("No accepted conversations yet.")).toBeInTheDocument();
  });
});

describe("message requests view", () => {
  it("shows the entry only with requests and opens the chat request card", async () => {
    await connected();
    const { container } = render(<RequestsHarness />);
    expect(container).toBeEmptyDOMElement();
    act(() => {
      api.mock.receivePmRequest(2, { chatRequest: true, note: "<b>Hi</b> & welcome" });
    });
    const entry = screen.getByRole("button", { name: /^Message requests/ });
    expect(entry).toHaveAccessibleName("Message requests 1 request");
    fireEvent.click(entry);
    expect(screen.getByRole("heading", { name: "Anna wants to chat with you" })).toBeVisible();
    // The note is untrusted plain text: shown as typed, never parsed as HTML.
    const note = screen.getByTestId("request-note");
    expect(note).toHaveTextContent("<b>Hi</b> & welcome");
    expect(note.querySelector("b")).toBeNull();
    expect(screen.getByText("Accept the request to reply to Anna.")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /Anna/ })).toHaveAttribute("aria-selected", "true");
  });

  it("accepts and declines through the engine", async () => {
    await connected();
    const first = api.mock.receivePmRequest(2, { chatRequest: true, note: null });
    const second = api.mock.receivePmRequest(4, { chatRequest: true, note: null });
    const accept = vi.spyOn(api, "pmRequestAccept");
    const decline = vi.spyOn(api, "pmRequestDecline");
    render(<RequestsHarness />);
    fireEvent.click(await screen.findByRole("button", { name: /^Message requests/ }));
    fireEvent.click(screen.getByRole("button", { name: "Decline" }));
    expect(decline).toHaveBeenCalledWith(shownServerId(), first.id);
    await screen.findByRole("heading", { name: "Carla wants to chat with you" });
    fireEvent.click(screen.getByRole("button", { name: "Accept" }));
    expect(accept).toHaveBeenCalledWith(shownServerId(), second.id);
  });

  it("moves between requests with the arrow keys", async () => {
    await connected();
    api.mock.receivePmRequest(2, { chatRequest: true, note: null });
    api.mock.receivePmRequest(4, { chatRequest: true, note: null });
    render(<RequestsHarness />);
    fireEvent.click(await screen.findByRole("button", { name: /^Message requests/ }));
    fireEvent.keyDown(screen.getByRole("tab", { name: /Anna/ }), { key: "ArrowRight" });
    expect(screen.getByRole("tab", { name: /Carla/ })).toHaveFocus();
    expect(screen.getByRole("heading", { name: "Carla wants to chat with you" })).toBeVisible();
  });

  it("shows held messages and adds a friend before accepting", async () => {
    await connected();
    const request = api.mock.receivePmRequest(2, {
      chatRequest: false,
      note: null,
      messages: ["<p>first</p>", "<p>second</p>"],
    });
    const friendAdd = vi.spyOn(api, "friendAdd");
    const accept = vi.spyOn(api, "pmRequestAccept");
    render(<RequestsHarness />);
    fireEvent.click(await screen.findByRole("button", { name: /^Message requests/ }));
    expect(screen.getByRole("heading", { name: "Anna sent you 2 messages" })).toBeVisible();
    expect(screen.getByText("first")).toBeInTheDocument();
    expect(screen.getByText("second")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add friend" }));
    await waitFor(() => {
      expect(accept).toHaveBeenCalledWith(shownServerId(), request.id);
    });
    expect(friendAdd).toHaveBeenCalledWith(shownServerId(), 2);
  });
});

describe("chat request from the person menu", () => {
  it("sends a note and then shows the request as pending", async () => {
    await connected();
    annaAcceptsChatRequests();
    const send = vi.spyOn(api, "chatRequestSend");
    render(
      <>
        <PersonMenuHost />
        <ChatRequestDialogHost />
      </>,
    );
    act(() => {
      openPersonMenu({ session: 2, x: 10, y: 10, opener: null });
    });
    fireEvent.click(screen.getByRole("menuitem", { name: "Send chat request…" }));
    const dialog = screen.getByRole("dialog", { name: "Chat request to Anna" });
    fireEvent.change(within(dialog).getByLabelText("Note (optional)"), {
      target: { value: "  Hello there " },
    });
    expect(within(dialog).getByText("14 of 200 characters used")).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Send" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(send).toHaveBeenCalledWith(shownServerId(), 2, "Hello there");
    act(() => {
      openPersonMenu({ session: 2, x: 10, y: 10, opener: null });
    });
    expect(screen.getByRole("menuitem", { name: "Chat request pending" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("is not offered to standard clients", async () => {
    await connected();
    render(<PersonMenuHost />);
    act(() => {
      openPersonMenu({ session: 2, x: 10, y: 10, opener: null });
    });
    expect(screen.queryByRole("menuitem", { name: /chat request/i })).toBeNull();
  });
});

describe("private message settings", () => {
  function renderGroup(initial: Settings["privateMessages"]) {
    const update = vi.fn();
    const settings = { privateMessages: initial } as Settings;
    render(<PrivateMessagesGroup settings={settings} update={update} />);
    return update;
  }

  const DEFAULTS: Settings["privateMessages"] = {
    rule: "everyone",
    serverRules: [],
    allowCurrentChannel: true,
    allowAccepted: true,
    keepRequests: true,
  };

  it("chooses the rule with labelled radio cards", async () => {
    await connected();
    const update = renderGroup(DEFAULTS);
    const friendsOnly = screen.getByRole("radio", { name: "Friends only" });
    expect(friendsOnly).toHaveAccessibleDescription(/chat request instead/);
    fireEvent.click(friendsOnly);
    expect(update).toHaveBeenCalledWith({
      privateMessages: { ...DEFAULTS, rule: "friendsOnly" },
    });
  });

  it("overrides the rule per saved server and goes back to the default", async () => {
    await connected();
    const update = renderGroup({ ...DEFAULTS, rule: "friendsOnly" });
    const select = await screen.findByRole("combobox", { name: /^Private messages from on / });
    expect(select).toHaveTextContent("Use default (Friends only)");
    chooseOption(select, "Nobody");
    const serverId = shownServerId();
    expect(update).toHaveBeenLastCalledWith({
      privateMessages: {
        ...DEFAULTS,
        rule: "friendsOnly",
        serverRules: [{ serverId, rule: "nobody" }],
      },
    });
  });

  it("removes an override when the default is chosen again", async () => {
    await connected();
    const serverId = shownServerId();
    const update = renderGroup({ ...DEFAULTS, serverRules: [{ serverId, rule: "nobody" }] });
    const select = await screen.findByRole("combobox", { name: /^Private messages from on / });
    chooseOption(select, "Use default (Everyone)");
    expect(update).toHaveBeenLastCalledWith({ privateMessages: DEFAULTS });
  });

  it("toggles the exceptions", async () => {
    await connected();
    const update = renderGroup(DEFAULTS);
    fireEvent.click(screen.getByRole("switch", { name: "People in my current channel" }));
    expect(update).toHaveBeenLastCalledWith({
      privateMessages: { ...DEFAULTS, allowCurrentChannel: false },
    });
  });
});
