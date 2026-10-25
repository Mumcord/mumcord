import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import { getServerState } from "../channels/serverStore";
import { getSelectedConversation, selectConversation } from "../channels/selection";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { MembersPanel } from "./MembersPanel";
import { joinTarget } from "./joinTarget";
import { PersonMenuHost } from "./PersonMenu";
import { closePersonMenu, openPersonMenu } from "./personMenuStore";
import { ProfilePreviewHost } from "./ProfilePreview";
import { VOLUME_DEBOUNCE_MS } from "./ProfileCard";

let api: MockApi;

async function renderPanel() {
  api = await connectedMock();
  render(
    <>
      <MembersPanel />
      <ProfilePreviewHost />
      <PersonMenuHost />
    </>,
  );
  await screen.findByRole("heading", { name: "In Lobby (3)" });
}

const member = (name: RegExp) => screen.getByRole("button", { name });
const menu = () => screen.queryByRole("menu");

beforeEach(stubDialog);
afterEach(() => {
  act(closePersonMenu);
  vi.useRealTimers();
  resetAll();
});

describe("PersonMenu", () => {
  it("opens on right-click with the four actions and focuses the first", async () => {
    await renderPanel();
    fireEvent.contextMenu(member(/^Anna/), { clientX: 30, clientY: 40 });
    expect(menu()).toHaveAccessibleName(/Anna/);
    expect(screen.getAllByRole("menuitem").map((i) => i.textContent)).toEqual([
      "View profile",
      "Send private message",
    ]);
    expect(screen.getByRole("menuitemcheckbox", { name: "Mute for me" })).toHaveAttribute(
      "aria-checked",
      "false",
    );
    expect(screen.getByRole("slider", { name: "Volume" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "View profile" })).toHaveFocus();
  });

  it("lists profile and message first, then a separator, then the settings", async () => {
    await renderPanel();
    fireEvent.contextMenu(member(/^Anna/), { clientX: 30, clientY: 40 });
    const entries = [
      ...(menu()?.querySelectorAll(
        '[role="menuitem"], [role="menuitemcheckbox"], [role="separator"], input[type="range"]',
      ) ?? []),
    ];
    expect(
      entries.map((entry) => entry.getAttribute("role") ?? entry.getAttribute("type")),
    ).toEqual([
      "menuitem",
      "menuitem",
      "separator",
      "menuitemcheckbox",
      "menuitemcheckbox",
      "range",
    ]);
    expect(entries[0]).toHaveTextContent("View profile");
    expect(entries[1]).toHaveTextContent("Send private message");
  });

  it("shows a decorative icon on every entry", async () => {
    await renderPanel();
    fireEvent.contextMenu(member(/^Anna/), { clientX: 30, clientY: 40 });
    const icons = [...(menu()?.querySelectorAll("svg") ?? [])];
    expect(icons).toHaveLength(5);
    for (const icon of icons) expect(icon).toHaveAttribute("aria-hidden", "true");
  });

  it("opens from the ContextMenu key and Shift+F10, Escape returns focus", async () => {
    await renderPanel();
    member(/^Anna/).focus();
    fireEvent.keyDown(member(/^Anna/), { key: "ContextMenu" });
    expect(menu()).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("menuitem", { name: "View profile" }), { key: "Escape" });
    expect(menu()).not.toBeInTheDocument();
    expect(member(/^Anna/)).toHaveFocus();
    fireEvent.keyDown(member(/^Anna/), { key: "F10", shiftKey: true });
    expect(menu()).toBeInTheDocument();
  });

  it("navigates with arrow keys, including the slider", async () => {
    await renderPanel();
    fireEvent.contextMenu(member(/^Anna/), { clientX: 1, clientY: 1 });
    const keys = (key: string) => fireEvent.keyDown(document.activeElement as Element, { key });
    keys("ArrowDown");
    expect(screen.getByRole("menuitem", { name: "Send private message" })).toHaveFocus();
    keys("ArrowDown");
    expect(screen.getByRole("menuitemcheckbox", { name: "Mute for me" })).toHaveFocus();
    keys("ArrowDown");
    expect(screen.getByRole("menuitemcheckbox", { name: /^Friend/ })).toHaveFocus();
    keys("ArrowDown");
    expect(screen.getByRole("slider")).toHaveFocus();
    keys("ArrowDown");
    expect(screen.getByRole("menuitem", { name: "View profile" })).toHaveFocus();
  });

  it("toggles local mute in place and keeps the menu open", async () => {
    await renderPanel();
    const spy = vi.spyOn(api, "personSetLocalMute");
    fireEvent.contextMenu(member(/^Anna/), { clientX: 1, clientY: 1 });
    const row = screen.getByRole("menuitemcheckbox", { name: "Mute for me" });
    fireEvent.click(row);
    expect(spy).toHaveBeenCalledWith(2, true);
    await act(async () => {
      await Promise.resolve();
    });
    expect(menu()).toBeInTheDocument();
    expect(screen.getByRole("menuitemcheckbox", { name: "Mute for me" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "Mute for me" }));
    expect(spy).toHaveBeenLastCalledWith(2, false);
  });

  it("adds and removes a friend in place", async () => {
    await renderPanel();
    const add = vi.spyOn(api, "friendAdd");
    const remove = vi.spyOn(api, "friendRemove");
    fireEvent.contextMenu(member(/^Anna/), { clientX: 1, clientY: 1 });
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: /^Friend/ }));
    expect(add).toHaveBeenCalledWith(2);
    await act(async () => {
      await Promise.resolve();
    });
    const row = screen.getByRole("menuitemcheckbox", { name: /^Friend/ });
    expect(row).toHaveAttribute("aria-checked", "true");
    fireEvent.click(row);
    expect(remove).toHaveBeenCalledWith("annahash");
  });

  it("explains why people without a certificate can't be friends", async () => {
    await renderPanel();
    const add = vi.spyOn(api, "friendAdd");
    fireEvent.contextMenu(member(/^Ben/), { clientX: 1, clientY: 1 });
    const row = screen.getByRole("menuitemcheckbox", { name: /^Friend/ });
    expect(row).toHaveAttribute("aria-disabled", "true");
    expect(row).toHaveTextContent("without a certificate");
    fireEvent.click(row);
    expect(add).not.toHaveBeenCalled();
  });

  it("changes the volume with the slider", async () => {
    await renderPanel();
    const spy = vi.spyOn(api, "personSetVolume");
    fireEvent.contextMenu(member(/^Anna/), { clientX: 1, clientY: 1 });
    vi.useFakeTimers();
    fireEvent.change(screen.getByRole("slider"), { target: { value: "150" } });
    expect(screen.getByText("150 %")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(VOLUME_DEBOUNCE_MS);
    });
    expect(spy).toHaveBeenCalledWith(2, 150);
  });

  it("opens the private conversation", async () => {
    await renderPanel();
    fireEvent.contextMenu(member(/^Anna/), { clientX: 1, clientY: 1 });
    fireEvent.click(screen.getByRole("menuitem", { name: "Send private message" }));
    expect(getSelectedConversation()?.kind).toMatch(/^private/);
  });

  it("opens the profile card", async () => {
    await renderPanel();
    fireEvent.contextMenu(member(/^Anna/), { clientX: 1, clientY: 1 });
    fireEvent.click(screen.getByRole("menuitem", { name: "View profile" }));
    expect(menu()).not.toBeInTheDocument();
    expect(document.querySelector("dialog.profile-card")).toBeInTheDocument();
  });

  it("shows only 'View profile' for yourself", async () => {
    await renderPanel();
    const self = getServerState().users.get(getServerState().selfSession ?? -1);
    act(() => {
      selectConversation({ kind: "channel", channelId: self?.channelId ?? 1 });
    });
    fireEvent.contextMenu(screen.getByRole("button", { name: /\(you\)/i }), {
      clientX: 1,
      clientY: 1,
    });
    expect(screen.getAllByRole("menuitem")).toHaveLength(1);
    expect(screen.queryByRole("slider")).not.toBeInTheDocument();
  });

  it("closes on outside click and replaces an open menu with the next", async () => {
    await renderPanel();
    fireEvent.contextMenu(member(/^Anna/), { clientX: 1, clientY: 1 });
    fireEvent.contextMenu(member(/^Ben/), { clientX: 1, clientY: 1 });
    expect(screen.getAllByRole("menu")).toHaveLength(1);
    expect(menu()).toHaveAccessibleName(/Ben/);
    fireEvent.pointerDown(document.body);
    expect(menu()).not.toBeInTheDocument();
  });

  it("closes the hover preview when it opens", async () => {
    await renderPanel();
    vi.useFakeTimers();
    const trigger = member(/^Anna/).parentElement as HTMLElement;
    fireEvent.pointerEnter(trigger, { pointerType: "mouse" });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(500);
    });
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
    fireEvent.contextMenu(trigger, { clientX: 1, clientY: 1 });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("stays open when a message arrives or another container scrolls", async () => {
    await renderPanel();
    fireEvent.contextMenu(member(/^Anna/), { clientX: 30, clientY: 40 });
    act(() => {
      api.mock.receiveMessage({ kind: "channel", channelId: 1 }, "hello", 3);
      document.body.dispatchEvent(new Event("scroll"));
    });
    expect(menu()).toBeInTheDocument();
    act(() => {
      document.dispatchEvent(new Event("scroll"));
    });
    expect(menu()).not.toBeInTheDocument();
  });

  describe("join the person's channel", () => {
    it("is hidden for someone in the same channel", async () => {
      await renderPanel();
      fireEvent.contextMenu(member(/^Anna/), { clientX: 1, clientY: 1 });
      expect(screen.queryByRole("menuitem", { name: /^Join/ })).not.toBeInTheDocument();
    });

    it("is hidden for yourself", async () => {
      await renderPanel();
      act(() => {
        openPersonMenu({ session: 1, x: 1, y: 1, opener: null });
      });
      expect(screen.queryByRole("menuitem", { name: /^Join/ })).not.toBeInTheDocument();
    });

    it("sits after the private message and moves you into their channel", async () => {
      await renderPanel();
      const join = vi.spyOn(api, "channelJoin");
      act(() => {
        openPersonMenu({ session: 4, x: 1, y: 1, opener: null });
      });
      const items = screen.getAllByRole("menuitem").map((i) => i.textContent);
      expect(items).toEqual(["View profile", "Send private message", "Join Raid"]);
      fireEvent.click(screen.getByRole("menuitem", { name: "Join Raid" }));
      expect(join).toHaveBeenCalledWith(2);
      expect(menu()).not.toBeInTheDocument();
    });
  });
});

describe("joinTarget", () => {
  it("disables the entry when the channel cannot be entered", async () => {
    api = await connectedMock();
    await waitFor(() => {
      expect(getServerState().users.get(4)).toBeDefined();
    });
    const state = getServerState();
    const carla = state.users.get(4);
    if (!carla) throw new Error("seed user missing");
    const gaming = state.channels.get(2);
    if (!gaming) throw new Error("seed channel missing");
    const denied = new Map(state.channels).set(2, {
      ...gaming,
      permissions: { ...gaming.permissions, enter: false },
    });
    expect(joinTarget({ ...state, channels: denied }, carla)?.allowed).toBe(false);
    expect(joinTarget(state, carla)?.allowed).toBe(true);
  });
});
