/** The media views of issue #163 sit under the chat header, not over it. */
import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../api";
import { selectConversation } from "../features/channels";
import { connectedMock, resetAll } from "../features/channels/testUtils";
import { resetMediaView, setFocus } from "../features/media/mediaView";
import { resetScreenShareStore } from "../features/screenshare/sharesStore";
import { resetWatchingStore, startWatching } from "../features/screenshare/watching";
import { ChatMount } from "./mounts";

// jsdom has no Tauri runtime; the frame channel only needs to exist for the viewer to render.
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: (message: ArrayBuffer) => void = () => undefined;
  },
}));

const ANNA = 2;

describe("chat mount", () => {
  let api: MockApi;

  beforeEach(async () => {
    api = await connectedMock({ bindStore: true });
    resetMediaView();
    resetWatchingStore();
    resetScreenShareStore();
    selectConversation({ kind: "channel", channelId: 1 });
    api.mock.emit("voiceServerChanged", { serverId: 1 });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    resetAll();
    resetMediaView();
    resetWatchingStore();
    resetScreenShareStore();
  });

  it("keeps the channel header above a watched share and the messages below it", async () => {
    startWatching(1, ANNA);
    const { container } = render(<ChatMount />);
    const header = await screen.findByRole("heading", { level: 2 });
    const frame = container.querySelector(".media-frame");
    const list = container.querySelector(".chat-list");

    expect(frame).not.toBeNull();
    expect(list).not.toBeNull();
    if (frame === null || list === null) return;
    expect(header.compareDocumentPosition(frame)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(frame.compareDocumentPosition(list)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it("shows the picture alone in focus mode", () => {
    startWatching(1, ANNA);
    setFocus(true);
    const { container } = render(<ChatMount />);

    expect(container.querySelector(".media-frame")).not.toBeNull();
    expect(container.querySelector(".chat-header")).toBeNull();
    expect(container.querySelector(".chat-list")).toBeNull();
  });
});
