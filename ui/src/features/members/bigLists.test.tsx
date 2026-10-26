/**
 * Showing a big server must not freeze the window (AC-2.17): long lists (everyone in other
 * channels) are drawn a step per frame, a long conversation only from its newest messages.
 */
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChannelInfo, ChatMessage, MockApi, User } from "../../api";
import { applyLanguage } from "../../i18n";
import { MessageList } from "../chat/MessageList";
import { mentionPeopleOf } from "../chat/mentionChips";
import { showServer } from "../channels/serverStore";
import { connectedMock, resetAll } from "../channels/testUtils";
import { BIG_SERVER_CHANNELS } from "../channels/tree";
import { MembersPanel } from "./MembersPanel";

const PEOPLE = 400;

afterEach(() => {
  vi.unstubAllGlobals();
  resetAll();
});

/** Frames run only when the test says so, to see what each frame draws. */
function manualFrames(): () => void {
  let queued: FrameRequestCallback[] = [];
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    queued.push(callback);
    return queued.length;
  });
  vi.stubGlobal("cancelAnimationFrame", () => undefined);
  return () => {
    const due = queued;
    queued = [];
    act(() => {
      for (const callback of due) callback(performance.now());
    });
  };
}

/** Connected to the seeded server 1 (shown) and a big server 2 with hundreds of people. */
async function bigSecondServer(): Promise<MockApi> {
  const api = await connectedMock({ bindStore: true });
  await api.serverAdd({
    label: "Big",
    address: "big.example.org",
    port: 64738,
    username: "Zoe",
    password: { kind: "clear" },
  });
  await api.connect(2, null, null);
  const snapshot = await api.sessionSnapshot(2);
  if (!snapshot) throw new Error("server 2 not connected");
  const template = snapshot.channels[0];
  const self = snapshot.users.find((u) => u.session === snapshot.selfSession);
  if (!template || !self) throw new Error("seeded server is empty");
  const channelCount = BIG_SERVER_CHANNELS * 4;
  const channels: ChannelInfo[] = [{ ...template, id: 0, parentId: null, name: "Root" }];
  for (let id = 1; id <= channelCount; id++) {
    channels.push({ ...template, id, parentId: 0, name: `Room ${String(id)}`, position: id });
  }
  const users: User[] = [{ ...self, channelId: 1 }];
  for (let i = 0; i < PEOPLE; i++) {
    users.push({
      ...self,
      session: 10_000 + i,
      name: `Person ${String(i)}`,
      channelId: 2 + (i % (channelCount - 1)),
    });
  }
  act(() => {
    api.mock.emit("serverSynced", { snapshot: { ...snapshot, channels, users } });
  });
  showServer(1);
  return api;
}

const memberRows = () => document.querySelectorAll(".member").length;

describe("switching to a big server", () => {
  it("draws everyone in other channels a step per frame", async () => {
    await bigSecondServer();
    render(<MembersPanel />);
    fireEvent.click(screen.getByRole("button", { name: /In other channels/ }));
    const nextFrame = manualFrames();

    act(() => {
      showServer(2);
    });
    // No frame draws everyone at once.
    let frames = 0;
    let drawn = memberRows();
    while (drawn < PEOPLE + 1 && frames < 100) {
      nextFrame();
      frames += 1;
      const now = memberRows();
      expect(now - drawn).toBeLessThanOrEqual(100);
      drawn = now;
    }
    expect(drawn).toBe(PEOPLE + 1);
  });
});

describe("a long conversation", () => {
  function message(index: number): ChatMessage {
    return {
      id: String(index),
      conversation: { kind: "channel", channelId: 1 },
      senderName: `Person ${String(index % 7)}`,
      senderCertHash: null,
      senderSession: null,
      own: false,
      sentAt: new Date(2026, 5, 15, 12, 0, index).toISOString(),
      body: `message ${String(index)}`,
      dismissedPreviews: [],
      syncedFrom: null,
    };
  }

  it("draws only the newest messages when shown, older ones on request", async () => {
    await applyLanguage("en");
    const messages = Array.from({ length: 90 }, (_, i) => message(i));
    render(
      <MessageList
        log={{
          messages,
          loaded: true,
          loading: false,
          hasMore: false,
          loadFailed: false,
          syncing: false,
          syncResult: null,
          syncFailed: false,
        }}
        label="Messages in #Lobby"
        onLoadOlder={() => undefined}
        listKey="2:lobby"
      />,
    );
    const items = () => screen.getAllByRole("listitem");
    expect(items()).toHaveLength(30);
    expect(items()[0]).toHaveTextContent("message 60");
    expect(items().at(-1)).toHaveTextContent("message 89");
    const older = screen.getByRole("button", { name: "Load older messages" });
    for (let page = 0; page < 2; page++) fireEvent.click(older);
    expect(items()).toHaveLength(90);
    expect(items()[0]).toHaveTextContent("message 0");
    expect(screen.queryByRole("button", { name: "Load older messages" })).not.toBeInTheDocument();
  });

  it("works out the people for mention chips once per people map", () => {
    const users = new Map<number, User>();
    expect(mentionPeopleOf(users)).toBe(mentionPeopleOf(users));
    expect(mentionPeopleOf(new Map(users))).not.toBe(mentionPeopleOf(users));
  });
});
