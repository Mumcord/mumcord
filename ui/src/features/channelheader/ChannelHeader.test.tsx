import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from "vitest";
import type { MockApi } from "../../api";
import { axeViolations } from "../../test/axe";
import { selectConversation } from "../channels";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { ChatView } from "../chat";
import { resetChatStore } from "../chat/chatStore";
import { FollowHeaderActions, resetFollowStore } from "../follow";
import { ChannelHeader } from ".";

let api: MockApi;
let copyText: MockInstance<MockApi["copyText"]>;

async function setup(channelId = 1) {
  resetFollowStore();
  api = await connectedMock({ bindStore: true });
  resetChatStore();
  stubDialog();
  copyText = vi.spyOn(api, "copyText").mockResolvedValue(null);
  const view = render(
    <ChatView
      renderChannelTitle={(id) => <ChannelHeader channelId={id} />}
      renderHeaderActions={(c) => <FollowHeaderActions conversation={c} />}
    />,
  );
  act(() => {
    selectConversation({ kind: "channel", channelId });
  });
  const heading = await screen.findByRole("heading", { level: 2 });
  const name = within(heading).getByRole("button");
  return { view, name };
}

function openMenu(name: HTMLElement) {
  fireEvent.click(name);
  return screen.getByRole("menu");
}

beforeEach(() => {
  resetFollowStore();
});

afterEach(() => {
  resetAll();
});

describe("channel header", () => {
  it("shows the name as a menu button and no separate Follow or Clear history buttons", async () => {
    const { view, name } = await setup();
    expect(name).toHaveAttribute("aria-haspopup", "menu");
    expect(name).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByRole("heading", { level: 2 })).toContainElement(name);
    expect(screen.queryByRole("button", { name: "Clear history" })).toBeNull();
    expect(screen.getByRole("button", { name: "Follow channel" })).toBeInTheDocument();
    expect(await axeViolations(view.container)).toEqual([]);
  });

  it("opens the menu from the keyboard and returns focus on Escape", async () => {
    const { name } = await setup();
    name.focus();
    fireEvent.keyDown(name, { key: "ContextMenu" });
    const menu = screen.getByRole("menu", { name: "Channel options for Lobby" });
    expect(name).toHaveAttribute("aria-expanded", "true");
    const items = within(menu).getAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual([
      "Follow channel",
      "Copy channel linkmumble://",
      "Channel info",
      "Clear chat history…",
    ]);
    expect(items[0]).toHaveFocus();
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(items[1]).toHaveFocus();
    expect(await axeViolations(menu)).toEqual([]);
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(name).toHaveFocus();
  });

  it("follows from the menu and the eye shows the state", async () => {
    const { name } = await setup();
    fireEvent.click(within(openMenu(name)).getByRole("menuitem", { name: "Follow channel" }));
    const eye = await screen.findByRole("button", { name: "Following, click to unfollow" });
    expect(eye).toHaveAttribute("aria-pressed", "true");
    expect(
      within(openMenu(name)).getByRole("menuitem", { name: /Unfollow channel/ }),
    ).toHaveTextContent("Following");
  });

  it("copies a mumble:// link without credentials", async () => {
    const { name } = await setup();
    fireEvent.click(within(openMenu(name)).getByRole("menuitem", { name: /Copy channel link/ }));
    await waitFor(() => {
      expect(copyText).toHaveBeenCalledWith(expect.stringMatching(/^mumble:\/\/[^@/]+\/Lobby\?/));
    });
    expect(await screen.findByText("Link copied")).toHaveAttribute("role", "status");
    expect(name).toHaveFocus();
  });

  it("clears the chat history after confirmation", async () => {
    const { name } = await setup();
    const clear = vi.spyOn(api, "historyClear");
    fireEvent.click(within(openMenu(name)).getByRole("menuitem", { name: "Clear chat history…" }));
    const dialog = screen.getByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Clear history" }));
    await waitFor(() => {
      expect(clear).toHaveBeenCalledWith({
        kind: "conversation",
        serverId: 1,
        conversation: { kind: "channel", channelId: 1 },
      });
    });
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(name).toHaveFocus();
  });

  it("offers the channel password only for a locked channel", async () => {
    const { name } = await setup(2);
    expect(within(openMenu(name)).queryByRole("menuitem", { name: /password/ })).toBeNull();
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    act(() => {
      api.mock.lockChannel(2);
    });
    const join = vi.spyOn(api, "channelJoin");
    fireEvent.click(within(openMenu(name)).getByRole("menuitem", { name: "Channel password…" }));
    expect(join).toHaveBeenCalledWith(1, 2);
  });

  it("shows channel info with the sanitised description and limits", async () => {
    const { name } = await setup();
    fireEvent.click(within(openMenu(name)).getByRole("menuitem", { name: "Channel info" }));
    const dialog = screen.getByRole("dialog", { name: "Channel info: Lobby" });
    expect(dialog.querySelector("script, [onerror]")).toBeNull();
    const row = (label: string) => within(dialog).getByText(label).nextElementSibling;
    expect(row("Users")).toHaveTextContent(/\d+ (person|people)/);
    expect(row("Type")).toHaveTextContent("Permanent");
    expect(row("Access")).toHaveTextContent("Open");
    expect(row("My permissions")).toHaveTextContent(
      "Enter, Speak, Text, Whisper, Listen, no Write ACL",
    );
    expect(row("Quality limit")).toHaveTextContent("Opus, up to 128 kbit/s");
    expect(row("History last cleared")).toHaveTextContent("Never");
    expect(await axeViolations(dialog)).toEqual([]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Copy link" }));
    expect(await within(dialog).findByRole("button", { name: "Copied" })).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(name).toHaveFocus();
  });
});
