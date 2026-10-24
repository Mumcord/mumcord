import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import { getServerState } from "../channels/serverStore";
import { getSelectedConversation, selectConversation } from "../channels/selection";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { MembersPanel } from "./MembersPanel";
import { PersonMenuHost } from "./PersonMenu";
import { closePersonMenu } from "./personMenuStore";
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
      "Mute for me",
      "Send private message",
    ]);
    expect(screen.getByRole("slider", { name: "Volume" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "View profile" })).toHaveFocus();
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
    expect(screen.getByRole("menuitem", { name: "Mute for me" })).toHaveFocus();
    keys("ArrowDown");
    expect(screen.getByRole("slider")).toHaveFocus();
    keys("ArrowDown");
    expect(screen.getByRole("menuitem", { name: "Send private message" })).toHaveFocus();
    keys("ArrowDown");
    expect(screen.getByRole("menuitem", { name: "View profile" })).toHaveFocus();
  });

  it("mutes locally and offers unmute afterwards", async () => {
    await renderPanel();
    const spy = vi.spyOn(api, "personSetLocalMute");
    fireEvent.contextMenu(member(/^Anna/), { clientX: 1, clientY: 1 });
    fireEvent.click(screen.getByRole("menuitem", { name: "Mute for me" }));
    expect(spy).toHaveBeenCalledWith(2, true);
    expect(menu()).not.toBeInTheDocument();
    await act(async () => {
      await Promise.resolve();
    });
    fireEvent.contextMenu(member(/^Anna/), { clientX: 1, clientY: 1 });
    expect(screen.getByRole("menuitem", { name: "Unmute for me" })).toBeInTheDocument();
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
});
