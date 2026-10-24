import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import { axeViolations } from "../../test/axe";
import { applyThemeToDocument } from "../../theme/theme";
import { getSelectedConversation, selectConversation } from "../channels/selection";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { MembersPanel } from "./MembersPanel";
import { ProfileCard, VOLUME_DEBOUNCE_MS } from "./ProfileCard";

let api: MockApi;

async function renderPanel(props: Parameters<typeof MembersPanel>[0] = {}) {
  api = await connectedMock();
  const view = render(<MembersPanel {...props} />);
  await screen.findByRole("heading", { name: "In Lobby (3)" });
  return view;
}

const member = (name: RegExp) => screen.getByRole("button", { name });
const avatarImage = (name: RegExp) => member(name).querySelector(".avatar img");

beforeEach(stubDialog);
afterEach(() => {
  vi.useRealTimers();
  resetAll();
});

describe("MembersPanel", () => {
  it("lists the people of the selected channel, then listeners", async () => {
    await renderPanel();
    expect(screen.getByRole("heading", { name: "Listening (1)" })).toBeInTheDocument();
    const buttons = screen.getAllByRole("button");
    expect(buttons[0]).toHaveAccessibleName(/^Anna/);
    expect(buttons[3]).toHaveAccessibleName(/^Carla/);
    expect(member(/^Ben/)).toHaveAccessibleName(/not registered/);

    act(() => {
      selectConversation({ kind: "channel", channelId: 2 });
    });
    expect(screen.getByRole("heading", { name: "In Raid (1)" })).toBeInTheDocument();
  });

  it("follows talking state and uses one Tab stop", async () => {
    await renderPanel();
    act(() => {
      api.mock.setTalking(2, true);
    });
    expect(member(/^Anna/)).toHaveAttribute("data-talking", "true");
    expect(screen.getAllByRole("button").filter((b) => b.tabIndex === 0)).toHaveLength(1);
    member(/^Anna/).focus();
    fireEvent.keyDown(member(/^Anna/), { key: "End" });
    expect(member(/^Carla/)).toHaveFocus();
    fireEvent.keyDown(member(/^Carla/), { key: "ArrowUp" });
    expect(member(/^Zoe/)).toHaveFocus();
  });

  it("keeps the talking indicator off listener rows", async () => {
    await renderPanel();
    act(() => {
      api.mock.setTalking(4, true);
    });
    expect(member(/^Carla/)).toHaveAttribute("data-talking", "false");
    expect(member(/^Carla/)).not.toHaveAccessibleName(/talking/i);
  });

  it("keeps voice state off listener rows", async () => {
    await renderPanel();
    await act(async () => {
      await api.channelFollow(2);
      await api.setSelfDeaf(true);
    });
    act(() => {
      selectConversation({ kind: "channel", channelId: 2 });
    });
    await screen.findByRole("heading", { name: "Listening (1)" });
    const listeners = screen.getAllByRole("button", { name: /listening/i });
    const zoe = listeners.find((b) => b.getAttribute("data-key") === "l1");
    expect(zoe).toBeDefined();
    expect(zoe).not.toHaveAccessibleName(/deafened|muted/);
    expect(zoe?.querySelectorAll(".person-icon")).toHaveLength(1);
  });

  it("omits a listener row for a person already in the channel", async () => {
    await renderPanel();
    const self = getServerState().users.get(getServerState().selfSession ?? -1);
    await act(async () => {
      await api.channelFollow(self?.channelId ?? 1);
    });
    expect(screen.getByRole("heading", { name: "Listening (1)" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /^Zoe/ })).toHaveLength(1);
  });

  it("opens a profile card with a sanitized comment and local controls", async () => {
    await renderPanel();
    const anna = member(/^Anna/);
    fireEvent.click(anna);
    const dialog = await screen.findByRole("dialog", { name: "Anna" });
    expect(await screen.findByText("Hi, I am Anna.")).toBeInTheDocument();
    expect(dialog).toHaveAttribute("open");

    vi.useFakeTimers();
    const slider = screen.getByRole("slider", { name: "Volume" });
    fireEvent.change(slider, { target: { value: "150" } });
    fireEvent.change(slider, { target: { value: "180" } });
    expect(getServerState().users.get(2)?.volume).toBe(100);
    await act(() => vi.advanceTimersByTimeAsync(VOLUME_DEBOUNCE_MS));
    expect(getServerState().users.get(2)?.volume).toBe(180);
    vi.useRealTimers();

    fireEvent.click(screen.getByRole("checkbox", { name: "Mute for me" }));
    await waitFor(() => {
      expect(getServerState().users.get(2)?.localMute).toBe(true);
    });
    expect(screen.queryByText(/only last for this session/)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(member(/^Anna/)).toHaveFocus();
  });

  it("strips unsafe markup from comments", async () => {
    api = await connectedMock();
    vi.spyOn(api, "personProfile").mockResolvedValue({
      comment: '<p>Hello</p><img src="x" onerror="alert(1)"><script>alert(2)</script>',
      avatar: null,
    });
    getServerState();
    await waitFor(() => {
      expect(getServerState().serverId).toBe(1);
    });
    render(<ProfileCard session={2} onClose={() => undefined} />);
    expect(await screen.findByText("Hello")).toBeInTheDocument();
    const dialog = screen.getByRole("dialog");
    expect(dialog.querySelector("script")).toBeNull();
    expect(dialog.querySelector("[onerror]")).toBeNull();
  });

  it("notes session-only settings without a certificate and hides controls on the own card", async () => {
    await renderPanel();
    fireEvent.click(member(/^Ben/));
    await screen.findByRole("dialog", { name: "Ben" });
    expect(screen.getByText(/only last for this session/)).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

    fireEvent.click(member(/^Zoe/));
    await screen.findByRole("dialog", { name: "Zoe" });
    expect(screen.queryByRole("slider")).toBeNull();
    expect(screen.queryByRole("checkbox")).toBeNull();
    expect(screen.queryByRole("button", { name: "Send private message" })).toBeNull();
  });

  it("opens a private conversation by default or calls the override", async () => {
    await renderPanel();
    fireEvent.click(member(/^Anna/));
    fireEvent.click(await screen.findByRole("button", { name: "Send private message" }));
    expect(getSelectedConversation()).toEqual({ kind: "private", certHash: "annahash" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("calls a custom private-message handler", async () => {
    const onSend = vi.fn();
    await renderPanel({ onSendPrivateMessage: onSend });
    fireEvent.click(member(/^Ben/));
    fireEvent.click(await screen.findByRole("button", { name: "Send private message" }));
    expect(onSend).toHaveBeenCalledWith(expect.objectContaining({ session: 3 }));
  });

  it("closes the card when the person leaves", async () => {
    await renderPanel();
    fireEvent.click(member(/^Anna/));
    await screen.findByRole("dialog", { name: "Anna" });
    act(() => {
      api.mock.emit("userRemoved", { session: 2 });
    });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("shows avatar images when people have one, initials otherwise", async () => {
    await renderPanel();
    const img = await waitFor(() => {
      const found = avatarImage(/^Anna/);
      expect(found).not.toBeNull();
      return found;
    });
    expect(img).toHaveAttribute("src", expect.stringMatching(/^data:image\//));
    expect(img).toHaveAttribute("alt", "");
    expect(avatarImage(/^Ben/)).toBeNull();
    expect(member(/^Ben/).querySelector(".avatar")).toHaveAttribute("data-initial", "B");
  });

  it("caches avatars and refreshes them when hasAvatar changes", async () => {
    await renderPanel();
    const profile = vi.spyOn(api, "personProfile");
    const ben = getServerState().users.get(3);
    if (!ben) throw new Error("Ben missing");

    act(() => {
      api.mock.setTalking(2, true);
    });
    await waitFor(() => {
      expect(avatarImage(/^Anna/)).not.toBeNull();
    });
    expect(profile).not.toHaveBeenCalled();

    profile.mockResolvedValueOnce({ comment: null, avatar: "data:image/png;base64,QkVO" });
    act(() => {
      api.mock.emit("userUpserted", { user: { ...ben, hasAvatar: true } });
    });
    await waitFor(() => {
      expect(avatarImage(/^Ben/)).toHaveAttribute("src", "data:image/png;base64,QkVO");
    });
    expect(profile).toHaveBeenCalledExactlyOnceWith(3);

    act(() => {
      api.mock.emit("userUpserted", { user: { ...ben, hasAvatar: false } });
    });
    expect(avatarImage(/^Ben/)).toBeNull();
    expect(member(/^Ben/).querySelector(".avatar")).toHaveAttribute("data-initial", "B");

    profile.mockResolvedValueOnce({ comment: null, avatar: "data:image/png;base64,TkVX" });
    act(() => {
      api.mock.emit("userUpserted", { user: { ...ben, hasAvatar: true } });
    });
    await waitFor(() => {
      expect(avatarImage(/^Ben/)).toHaveAttribute("src", "data:image/png;base64,TkVX");
    });
    expect(profile).toHaveBeenCalledTimes(2);
  });

  it("refetches an avatar when only avatarHash changes", async () => {
    await renderPanel();
    const profile = vi.spyOn(api, "personProfile");
    const ben = getServerState().users.get(3);
    if (!ben) throw new Error("Ben missing");

    profile.mockResolvedValueOnce({ comment: null, avatar: "data:image/png;base64,QkVO" });
    act(() => {
      api.mock.emit("userUpserted", { user: { ...ben, hasAvatar: true, avatarHash: "aa" } });
    });
    await waitFor(() => {
      expect(avatarImage(/^Ben/)).toHaveAttribute("src", "data:image/png;base64,QkVO");
    });

    profile.mockResolvedValueOnce({ comment: null, avatar: "data:image/png;base64,TkVX" });
    act(() => {
      api.mock.emit("userUpserted", { user: { ...ben, hasAvatar: true, avatarHash: "bb" } });
    });
    await waitFor(() => {
      expect(avatarImage(/^Ben/)).toHaveAttribute("src", "data:image/png;base64,TkVX");
    });
    expect(profile).toHaveBeenCalledTimes(2);
  });

  it("falls back to initials when loading an avatar fails", async () => {
    api = await connectedMock();
    vi.spyOn(api, "personProfile").mockRejectedValue({ code: "notConnected", message: "x" });
    const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(<MembersPanel />);
    await screen.findByRole("heading", { name: "In Lobby (3)" });
    await waitFor(() => {
      expect(error).toHaveBeenCalled();
    });
    expect(avatarImage(/^Anna/)).toBeNull();
    expect(member(/^Anna/).querySelector(".avatar")).toHaveAttribute("data-initial", "A");
    error.mockRestore();
  });

  it("shows the status icons at the end of every row", async () => {
    await renderPanel();
    const rows = screen.getAllByRole("button").filter((b) => b.classList.contains("member"));
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) {
      expect(row.lastElementChild).toHaveClass("person-icons");
      expect(row.querySelectorAll(".person-icon").length).toBeGreaterThan(0);
    }
  });

  it.each(["dark", "light"] as const)("has no axe violations (%s)", async (theme) => {
    applyThemeToDocument(theme);
    const { container } = await renderPanel();
    await waitFor(() => {
      expect(avatarImage(/^Anna/)).not.toBeNull();
    });
    expect(await axeViolations(container)).toEqual([]);
    fireEvent.click(member(/^Anna/));
    await screen.findByText("Hi, I am Anna.");
    expect(await axeViolations(container)).toEqual([]);
  });
});
