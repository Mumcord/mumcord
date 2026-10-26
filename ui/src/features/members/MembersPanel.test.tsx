import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import { PreferencesContext, type Preferences } from "../../app/preferences";
import { axeViolations } from "../../test/axe";
import { applyThemeToDocument } from "../../theme/theme";
import { getSelectedConversation, selectConversation } from "../channels/selection";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { MembersPanel } from "./MembersPanel";
import { ProfilePreviewHost } from "./ProfilePreview";
import { PIN_DELAY_MS } from "./profilePreviewStore";
import { ProfileCard, VOLUME_THROTTLE_MS } from "./ProfileCard";

let api: MockApi;

async function renderPanel(props: Parameters<typeof MembersPanel>[0] = {}) {
  api = await connectedMock();
  const view = render(
    <>
      <MembersPanel {...props} />
      <ProfilePreviewHost />
    </>,
  );
  await screen.findByRole("heading", { name: "In Lobby (4)" });
  return view;
}

const member = (name: RegExp) => screen.getByRole("button", { name });
/** A single click; the card pins once the double-click window has passed. */
function pin(name: RegExp) {
  vi.useFakeTimers();
  fireEvent.click(member(name));
  act(() => {
    vi.advanceTimersByTime(PIN_DELAY_MS);
  });
  vi.useRealTimers();
}

function openFullProfile(name: RegExp) {
  pin(name);
  fireEvent.click(screen.getByRole("button", { name: "Full profile" }));
}

const avatarImage = (name: RegExp) => member(name).querySelector(".avatar img");

beforeEach(stubDialog);
afterEach(() => {
  vi.useRealTimers();
  resetAll();
});

describe("MembersPanel", () => {
  it("lists talking people first when that setting is on", async () => {
    api = await connectedMock();
    const preferences: Preferences = {
      theme: "system",
      resolvedTheme: "dark",
      language: "en",
      chatDisplay: "cozy",
      linkPreviews: false,
      hideEmptyChannels: false,
      talkingFirst: true,
      showWeakConnectionIcon: true,
      applySettings: () => undefined,
    };
    render(
      <PreferencesContext.Provider value={preferences}>
        <MembersPanel />
      </PreferencesContext.Provider>,
    );
    await screen.findByRole("heading", { name: "In Lobby (4)" });
    const names = () =>
      screen
        .getAllByRole("button")
        .filter((b) => b.classList.contains("member"))
        .map((b) => b.textContent);
    expect(names()[0]).toMatch(/^Anna/);
    const ben = [...getServerState().users.values()].find((u) => u.name === "Ben");
    if (!ben) throw new Error("seed has no Ben");
    act(() => {
      api.mock.emit("talkStateChanged", { serverId: 1, session: ben.session, talking: true });
    });
    await waitFor(() => {
      expect(names()[0]).toMatch(/^Ben/);
    });
    act(() => {
      api.mock.emit("talkStateChanged", { serverId: 1, session: ben.session, talking: false });
    });
    await waitFor(() => {
      expect(names()[0]).toMatch(/^Anna/);
    });
  });

  it("lists the people of the selected channel with listeners inline", async () => {
    await renderPanel();
    expect(screen.queryByRole("heading", { name: /listening/i })).not.toBeInTheDocument();
    const buttons = screen.getAllByRole("button").filter((b) => b.classList.contains("member"));
    expect(buttons[0]).toHaveAccessibleName(/^Anna/);
    expect(buttons[2]).toHaveAccessibleName(/^Carla.*listening only/i);
    expect(buttons[2]?.querySelector(".person-icon")).not.toBeNull();
    expect(member(/^Ben/)).toHaveAccessibleName(/not registered/);

    act(() => {
      selectConversation({ kind: "channel", channelId: 2 });
    });
    expect(screen.getByRole("heading", { name: "In Raid (1)" })).toBeInTheDocument();
  });

  it("collapses the sections and lists people of other channels, collapsed at first", async () => {
    await renderPanel();
    const others = screen.getByRole("button", { name: "In other channels" });
    expect(others).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("heading", { name: "Raid" })).not.toBeInTheDocument();
    fireEvent.click(others);
    expect(screen.getByRole("heading", { name: "Raid" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /^Carla/ })).toHaveLength(2);

    const here = screen.getByRole("button", { name: "In Lobby (4)" });
    fireEvent.click(here);
    expect(here).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("button", { name: /^Anna/ })).not.toBeInTheDocument();
    // The open state outlives the component, so put both sections back for the other tests.
    fireEvent.click(here);
    fireEvent.click(others);
  });

  it("follows talking state and uses one Tab stop", async () => {
    await renderPanel();
    act(() => {
      api.mock.setTalking(2, true);
    });
    expect(member(/^Anna/)).toHaveAttribute("data-talking", "true");
    expect(
      screen
        .getAllByRole("button")
        .filter((b) => b.classList.contains("member") && b.tabIndex === 0),
    ).toHaveLength(1);
    member(/^Anna/).focus();
    fireEvent.keyDown(member(/^Anna/), { key: "End" });
    expect(member(/^Zoe/)).toHaveFocus();
    fireEvent.keyDown(member(/^Zoe/), { key: "ArrowUp" });
    expect(member(/^Carla/)).toHaveFocus();
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
      await api.channelFollow(1, 2);
      await api.setSelfDeaf(true);
    });
    act(() => {
      selectConversation({ kind: "channel", channelId: 2 });
    });
    const listeners = await screen.findAllByRole("button", { name: /listening only/i });
    const zoe = listeners.find((b) => b.getAttribute("data-key") === "l1");
    expect(zoe).toBeDefined();
    expect(zoe).not.toHaveAccessibleName(/deafened|muted/);
    expect(zoe?.querySelectorAll(".person-icon")).toHaveLength(1);
  });

  it("omits a listener row for a person already in the channel", async () => {
    await renderPanel();
    const self = getServerState().users.get(getServerState().selfSession ?? -1);
    await act(async () => {
      await api.channelFollow(1, self?.channelId ?? 1);
    });
    expect(screen.getAllByRole("button", { name: /^Zoe/ })).toHaveLength(1);
  });

  it("opens a profile card with a sanitized comment and local controls", async () => {
    await renderPanel();
    openFullProfile(/^Anna/);
    const dialog = await screen.findByRole("dialog", { name: "Anna" });
    expect(await screen.findByText("Hi, I am Anna.")).toBeInTheDocument();
    expect(dialog).toHaveAttribute("open");

    fireEvent.click(screen.getByRole("button", { name: "Settings for Anna" }));
    vi.useFakeTimers();
    const slider = screen.getByRole("slider", { name: "Volume" });
    fireEvent.change(slider, { target: { value: "150" } });
    fireEvent.change(slider, { target: { value: "180" } });
    await act(() => vi.advanceTimersByTimeAsync(VOLUME_THROTTLE_MS));
    expect(getServerState().users.get(2)?.volume).toBe(180);
    vi.useRealTimers();

    fireEvent.click(screen.getByRole("switch", { name: "Mute for me" }));
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
      note: "",
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
    openFullProfile(/^Ben/);
    await screen.findByRole("dialog", { name: "Ben" });
    fireEvent.click(screen.getByRole("button", { name: "Settings for Ben" }));
    expect(screen.getByText(/only last for this session/)).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

    openFullProfile(/^Zoe/);
    await screen.findByRole("dialog", { name: "Zoe" });
    expect(screen.queryByRole("slider")).toBeNull();
    expect(screen.queryByRole("switch")).toBeNull();
    expect(screen.queryByRole("button", { name: "Private message" })).toBeNull();
  });

  it("opens the private chat on double click, or calls the override", async () => {
    await renderPanel();
    fireEvent.doubleClick(member(/^Anna/));
    expect(getSelectedConversation()).toEqual({ kind: "private", certHash: "annahash" });
    expect(screen.queryByRole("button", { name: "Full profile" })).toBeNull();
  });

  it("calls a custom private-message handler on double click", async () => {
    const onSend = vi.fn();
    await renderPanel({ onSendPrivateMessage: onSend });
    fireEvent.doubleClick(member(/^Ben/));
    expect(onSend).toHaveBeenCalledWith(expect.objectContaining({ session: 3 }));
  });

  it("does not pin a card when the click is the first half of a double click", () => {
    return renderPanel().then(() => {
      vi.useFakeTimers();
      fireEvent.click(member(/^Anna/), { detail: 1 });
      fireEvent.click(member(/^Anna/), { detail: 2 });
      fireEvent.doubleClick(member(/^Anna/));
      act(() => {
        vi.advanceTimersByTime(PIN_DELAY_MS * 2);
      });
      vi.useRealTimers();
      expect(screen.queryByRole("button", { name: "Full profile" })).toBeNull();
      expect(getSelectedConversation()).toEqual({ kind: "private", certHash: "annahash" });
    });
  });

  it("pins the card on click with both buttons; Escape closes it and returns focus", async () => {
    await renderPanel();
    pin(/^Anna/);
    const card = screen.getByRole("dialog", { name: "Anna" });
    expect(card).toHaveFocus();
    expect(screen.getByRole("button", { name: "Private message" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Full profile" })).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(member(/^Anna/)).toHaveFocus();
  });

  it("closes the pinned card with the close button and on an outside click", async () => {
    await renderPanel();
    pin(/^Anna/);
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    pin(/^Anna/);
    fireEvent.pointerDown(screen.getByRole("heading", { name: "In Lobby (4)" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("opens the private chat from the pinned card", async () => {
    await renderPanel();
    pin(/^Anna/);
    fireEvent.click(screen.getByRole("button", { name: "Private message" }));
    expect(getSelectedConversation()).toEqual({ kind: "private", certHash: "annahash" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("offers no private message for yourself", async () => {
    await renderPanel();
    const before = getSelectedConversation();
    pin(/^Zoe/);
    expect(screen.getByRole("button", { name: "Full profile" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Private message" })).toBeNull();
    fireEvent.doubleClick(member(/^Zoe/));
    expect(getSelectedConversation()).toEqual(before);
  });

  it("closes the card when the person leaves", async () => {
    await renderPanel();
    openFullProfile(/^Anna/);
    await screen.findByRole("dialog", { name: "Anna" });
    act(() => {
      api.mock.emit("userRemoved", { serverId: 1, session: 2 });
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

    profile.mockResolvedValueOnce({
      comment: null,
      avatar: "data:image/png;base64,QkVO",
      note: "",
    });
    act(() => {
      api.mock.emit("userUpserted", { serverId: 1, user: { ...ben, hasAvatar: true } });
    });
    await waitFor(() => {
      expect(avatarImage(/^Ben/)).toHaveAttribute("src", "data:image/png;base64,QkVO");
    });
    expect(profile).toHaveBeenCalledExactlyOnceWith(1, 3);

    act(() => {
      api.mock.emit("userUpserted", { serverId: 1, user: { ...ben, hasAvatar: false } });
    });
    expect(avatarImage(/^Ben/)).toBeNull();
    expect(member(/^Ben/).querySelector(".avatar")).toHaveAttribute("data-initial", "B");

    profile.mockResolvedValueOnce({
      comment: null,
      avatar: "data:image/png;base64,TkVX",
      note: "",
    });
    act(() => {
      api.mock.emit("userUpserted", { serverId: 1, user: { ...ben, hasAvatar: true } });
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

    profile.mockResolvedValueOnce({
      comment: null,
      avatar: "data:image/png;base64,QkVO",
      note: "",
    });
    act(() => {
      api.mock.emit("userUpserted", {
        serverId: 1,
        user: { ...ben, hasAvatar: true, avatarHash: "aa" },
      });
    });
    await waitFor(() => {
      expect(avatarImage(/^Ben/)).toHaveAttribute("src", "data:image/png;base64,QkVO");
    });

    profile.mockResolvedValueOnce({
      comment: null,
      avatar: "data:image/png;base64,TkVX",
      note: "",
    });
    act(() => {
      api.mock.emit("userUpserted", {
        serverId: 1,
        user: { ...ben, hasAvatar: true, avatarHash: "bb" },
      });
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
    await screen.findByRole("heading", { name: "In Lobby (4)" });
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
    openFullProfile(/^Anna/);
    await screen.findByText("Hi, I am Anna.");
    expect(await axeViolations(container)).toEqual([]);
  });
});
