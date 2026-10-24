import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Conversation, MockApi, NotificationMode } from "../../api";
import { axeViolations } from "../../test/axe";
import { getSelectedConversation, selectConversation } from "../channels";
import { connectedMock, resetAll } from "../channels/testUtils";
import { resetChatStore } from "../chat/chatStore";
import { MAX_PREVIEW, plainPreview } from "./preview";
import { TOAST_MS } from "./Toasts";
import { NotificationsHost, resetNotifications } from ".";

const LOBBY: Conversation = { kind: "channel", channelId: 1 };
const RAID: Conversation = { kind: "channel", channelId: 2 };
const AFK: Conversation = { kind: "channel", channelId: 3 };
const ANNA = 2;
let api: MockApi;

async function setup(options: { mode?: NotificationMode; overlayOpen?: boolean } = {}) {
  resetNotifications();
  api = await connectedMock();
  resetChatStore();
  if (options.mode) {
    const settings = await api.settingsGet();
    await api.settingsUpdate({ ...settings, notifications: options.mode });
  }
  const onOpenChat = vi.fn();
  const view = render(
    <>
      <main id="chat" tabIndex={-1}>
        <textarea aria-label="Message" />
      </main>
      <NotificationsHost overlayOpen={options.overlayOpen ?? false} onOpenChat={onOpenChat} />
    </>,
  );
  await waitFor(() => {
    expect(getSelectedConversation()).toEqual(LOBBY);
  });
  return { view, onOpenChat };
}

function receive(conversation: Conversation, body: string, session = ANNA) {
  act(() => {
    api.mock.receiveMessage(conversation, body, session);
  });
}

function toastRegion() {
  return screen.getByRole("region", { name: "Notifications" });
}

afterEach(() => {
  vi.useRealTimers();
  resetNotifications();
  resetAll();
});

describe("message toasts", () => {
  it("shows channel, sender and a plain preview for another chat, without axe violations", async () => {
    const { view } = await setup();
    receive(RAID, '<p><b>Raid</b> at <img src="x.png" alt="pic"> eight</p><script>x</script>');
    const toast = await within(toastRegion()).findByRole("button", { name: /#Raid/ });
    expect(toast).toHaveTextContent("#RaidAnna: Raid at eight x");
    expect(toastRegion().querySelector("img, b, script")).toBeNull();
    expect(within(toastRegion()).getByRole("status")).toHaveAttribute("aria-live", "polite");
    expect(await axeViolations(view.container)).toEqual([]);
  });

  it("stays silent for the chat on screen and for own messages", async () => {
    await setup();
    receive(LOBBY, "visible already");
    await act(async () => {
      await api.sendChannelMessage(2, "mine");
    });
    await act(() => Promise.resolve());
    expect(within(toastRegion()).queryByRole("button")).toBeNull();
  });

  it("notifies for the selected chat while settings cover it", async () => {
    await setup({ overlayOpen: true });
    receive(LOBBY, "hidden behind settings");
    expect(await within(toastRegion()).findByRole("button", { name: /#Lobby/ })).toBeVisible();
  });

  it("groups a burst per channel and keeps at most three toasts", async () => {
    await setup();
    receive(RAID, "one");
    receive(RAID, "two");
    receive(RAID, "three");
    expect(
      await within(toastRegion()).findByRole("button", { name: /3 new messages in #Raid/ }),
    ).toHaveTextContent("Anna: three");
    receive(AFK, "afk");
    receive({ kind: "privateSession", session: 3 }, "psst", 3);
    receive({ kind: "private", certHash: "annahash" }, "hi", ANNA);
    await waitFor(() => {
      expect(within(toastRegion()).getAllByRole("button", { name: /^(?!Dismiss)/ })).toHaveLength(
        3,
      );
    });
    expect(within(toastRegion()).queryByRole("button", { name: /#Raid/ })).toBeNull();
    expect(within(toastRegion()).getByRole("button", { name: /^Ben/ })).toHaveTextContent("psst");
  });

  it("opens and focuses the chat on click and forgets the count", async () => {
    const { onOpenChat } = await setup();
    receive(RAID, "one");
    receive(RAID, "two");
    fireEvent.click(await within(toastRegion()).findByRole("button", { name: /#Raid/ }));
    expect(onOpenChat).toHaveBeenCalled();
    expect(getSelectedConversation()).toEqual(RAID);
    await waitFor(() => {
      expect(screen.getByRole("textbox", { name: "Message" })).toHaveFocus();
    });
    expect(within(toastRegion()).queryByRole("button")).toBeNull();
    act(() => {
      selectConversation(LOBBY);
    });
    receive(RAID, "three");
    expect(await within(toastRegion()).findByRole("button", { name: /^#Raid/ })).toHaveTextContent(
      "Anna: three",
    );
  });

  it("opens the chat when a desktop notification is clicked", async () => {
    await setup();
    act(() => {
      api.mock.emit("notificationActivated", { conversation: RAID });
    });
    expect(getSelectedConversation()).toEqual(RAID);
  });

  it("closes with the button and disappears after a while unless hovered", async () => {
    await setup();
    receive(RAID, "one");
    const close = await within(toastRegion()).findByRole("button", {
      name: "Dismiss notification",
    });
    fireEvent.click(close);
    expect(within(toastRegion()).queryByRole("button")).toBeNull();

    vi.useFakeTimers();
    receive(AFK, "afk");
    await act(() => vi.advanceTimersByTimeAsync(0));
    fireEvent.mouseEnter(toastRegion());
    await act(() => vi.advanceTimersByTimeAsync(TOAST_MS * 2));
    expect(within(toastRegion()).getByRole("button", { name: /#AFK/ })).toBeInTheDocument();
    fireEvent.mouseLeave(toastRegion());
    await act(() => vi.advanceTimersByTimeAsync(TOAST_MS));
    expect(within(toastRegion()).queryByRole("button")).toBeNull();
  });
});

describe("notification setting", () => {
  it("off shows nothing", async () => {
    await setup({ mode: "off" });
    receive(RAID, "quiet");
    await act(() => Promise.resolve());
    expect(within(toastRegion()).queryByRole("button")).toBeNull();
    expect(api.mock.desktopNotices()).toEqual([]);
  });

  it("desktop sends a system notification instead of a toast", async () => {
    await setup({ mode: "desktop" });
    receive(RAID, "one");
    receive(RAID, "<i>two</i>");
    await waitFor(() => {
      expect(api.mock.desktopNotices()).toHaveLength(2);
    });
    expect(api.mock.desktopNotices()[1]).toMatchObject({
      conversation: RAID,
      onScreen: false,
      summary: "2 new messages in #Raid",
      body: "Anna: two",
    });
    expect(within(toastRegion()).queryByRole("button")).toBeNull();
  });

  it("both uses the system notification while the window is in the background", async () => {
    await setup();
    api.mock.setWindowFocused(false);
    receive(LOBBY, "selected but not seen");
    await waitFor(() => {
      expect(api.mock.desktopNotices()).toHaveLength(1);
    });
    expect(within(toastRegion()).queryByRole("button")).toBeNull();
  });

  it("in-app shows nothing while the window is in the background", async () => {
    await setup({ mode: "inApp" });
    api.mock.setWindowFocused(false);
    receive(RAID, "later");
    await act(() => Promise.resolve());
    expect(within(toastRegion()).queryByRole("button")).toBeNull();
    expect(api.mock.desktopNotices()).toEqual([]);
  });
});

describe("plain preview", () => {
  it("keeps only text and shortens long messages", () => {
    expect(plainPreview("<p>a</p><p>b<br>c</p>")).toBe("a b c");
    expect(plainPreview('<img src="data:image/png;base64,AAA">')).toBe("");
    const long = plainPreview(`<p>${"x".repeat(500)}</p>`);
    expect(Array.from(long)).toHaveLength(MAX_PREVIEW);
    expect(long.endsWith("…")).toBe(true);
  });
});
