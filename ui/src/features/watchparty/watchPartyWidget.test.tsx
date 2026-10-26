import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, WatchParty } from "../../api";
import { MediaWidgetsMount } from "../../app/mounts";
import * as chat from "../chat";
import { resetFloatingWidgets } from "../../components/floatingWidgetStore";
import { getServerState, showServer } from "../channels/serverStore";
import { connectedMock, resetAll } from "../channels/testUtils";
import { resetWatchPartyStore } from "./watchPartyStore";

function party(patch: Partial<WatchParty>): WatchParty {
  return {
    channelId: 0,
    videoId: "dQw4w9WgXcQ",
    playing: true,
    positionMs: 0,
    participants: [],
    lastAction: null,
    joined: false,
    currentEntry: 0,
    playlist: [],
    skippedBy: null,
    ...patch,
  };
}

describe("watch party widget", () => {
  let api: MockApi;
  let channelId: number;

  beforeEach(async () => {
    api = await connectedMock({ bindStore: true });
    const { users, selfSession } = getServerState();
    channelId = users.get(selfSession ?? -1)?.channelId ?? -1;
    act(() => {
      api.mock.emit("voiceServerChanged", { serverId: 1 });
    });
  });
  afterEach(() => {
    resetWatchPartyStore();
    resetFloatingWidgets();
    resetAll();
  });

  const announce = (p: WatchParty | null) => {
    act(() => {
      api.mock.emit("watchPartyChanged", { serverId: 1, party: p });
    });
  };

  it("opens only once joined and closes on leave or when the party ends", () => {
    render(<MediaWidgetsMount />);
    announce(party({ channelId }));
    expect(document.querySelector(".floating-widget")).toBeNull();
    expect(document.querySelector("iframe")).toBeNull();
    announce(party({ channelId, joined: true }));
    expect(document.querySelector(".floating-widget")).toHaveAccessibleName("Watch party");
    announce(party({ channelId, joined: false }));
    expect(document.querySelector(".floating-widget")).toBeNull();
    announce(party({ channelId, joined: true }));
    announce(null);
    expect(document.querySelector(".floating-widget")).toBeNull();
  });

  it("collapses to the title with play/pause", () => {
    const command = vi.spyOn(api, "watchPartyCommand").mockResolvedValue(null);
    render(<MediaWidgetsMount />);
    announce(party({ channelId, joined: true, playing: false, positionMs: 5_000 }));
    fireEvent.click(screen.getByRole("button", { name: "Collapse" }));
    fireEvent.click(screen.getByRole("button", { name: "Play" }));
    expect(command).toHaveBeenCalledWith(1, { kind: "play", positionMs: 5_000 });
  });

  it("adds a video from the + button, also while collapsed, on the voice server", () => {
    const add = vi.spyOn(api, "watchPartyAdd").mockResolvedValue(0);
    render(<MediaWidgetsMount />);
    announce(party({ channelId, joined: true }));
    act(() => {
      showServer(2);
    });
    fireEvent.click(screen.getByRole("button", { name: "Collapse" }));
    fireEvent.click(screen.getByRole("button", { name: "Add video" }));
    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "https://youtu.be/dQw4w9WgXcQ" },
    });
    fireEvent.submit(screen.getByRole("textbox"));
    expect(add).toHaveBeenCalledWith(1, "dQw4w9WgXcQ");
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  it("renders the add-video dialog outside the widget so it cannot be trapped behind it", () => {
    render(<MediaWidgetsMount />);
    announce(party({ channelId, joined: true }));
    fireEvent.click(screen.getByRole("button", { name: "Add video" }));
    const dialog = screen.getByRole("dialog");
    expect(document.querySelector(".floating-widget")?.contains(dialog)).toBe(false);
    expect(dialog.closest(".settings-dialog__backdrop")?.parentElement).toBe(document.body);
  });

  it("does not drag the widget when + is pressed", () => {
    render(<MediaWidgetsMount />);
    announce(party({ channelId, joined: true }));
    const widget = document.querySelector<HTMLElement>(".floating-widget");
    const before = widget?.style.left;
    const plus = screen.getByRole("button", { name: "Add video" });
    fireEvent.pointerDown(plus, { button: 0, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(window, { clientX: 80, clientY: 80 });
    fireEvent.pointerUp(window);
    expect(widget?.style.left).toBe(before);
    expect(widget).not.toHaveAttribute("data-gesture");
  });

  it("shows joined participants as avatars with +N overflow and a label", () => {
    render(<MediaWidgetsMount />);
    const self = [...getServerState().users.values()][0];
    if (self === undefined) throw new Error("no user");
    const sessions = [101, 102, 103, 104, 105];
    act(() => {
      for (const session of sessions) {
        api.mock.emit("userUpserted", {
          serverId: 1,
          user: { ...self, session, name: `User${String(session)}` },
        });
      }
    });
    announce(party({ channelId, joined: true, participants: sessions }));
    const names = sessions.map((session) => `User${String(session)}`);
    const group = screen.getByRole("img", { name: `5 watching: ${names.join(", ")}` });
    expect(group.querySelectorAll(".avatar")).toHaveLength(3);
    expect(group).toHaveTextContent("+2");
    expect(group.querySelector(".watch-party-widget__person")).toHaveAttribute(
      "data-tooltip",
      "User101",
    );
  });

  it("leaves from the title bar, expanded and collapsed", () => {
    const leave = vi.spyOn(api, "watchPartyLeave").mockResolvedValue(null);
    render(<MediaWidgetsMount />);
    announce(party({ channelId, joined: true }));
    fireEvent.click(screen.getByRole("button", { name: "Leave party" }));
    expect(leave).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Collapse" }));
    fireEvent.click(screen.getByRole("button", { name: "Leave party" }));
    expect(leave).toHaveBeenCalledTimes(2);
    expect(leave).toHaveBeenCalledWith(1);
  });

  it("does not drag when Leave or the avatars are pressed", () => {
    render(<MediaWidgetsMount />);
    announce(party({ channelId, joined: true, participants: [2] }));
    const widget = document.querySelector<HTMLElement>(".floating-widget");
    const before = widget?.style.left;
    const targets = [
      screen.getByRole("button", { name: "Leave party" }),
      screen.getByRole("img", { name: /watching/ }),
    ];
    for (const target of targets) {
      fireEvent.pointerDown(target, { button: 0, clientX: 10, clientY: 10 });
      fireEvent.pointerMove(window, { clientX: 80, clientY: 80 });
      fireEvent.pointerUp(window);
      expect(widget?.style.left).toBe(before);
      expect(widget).not.toHaveAttribute("data-gesture");
    }
  });

  it("stays on the voice server's party when another server is shown", () => {
    const command = vi.spyOn(api, "watchPartyCommand").mockResolvedValue(null);
    render(<MediaWidgetsMount />);
    announce(party({ channelId, joined: true, playing: false, positionMs: 5_000 }));
    act(() => {
      showServer(2);
    });
    fireEvent.click(screen.getByRole("button", { name: "Collapse" }));
    fireEvent.click(screen.getByRole("button", { name: "Play" }));
    expect(command).toHaveBeenCalledWith(1, { kind: "play", positionMs: 5_000 });
  });
});

describe("chat dock", () => {
  it("is gone: media no longer docks below the chat", () => {
    expect("ChatDock" in chat).toBe(false);
  });
});
