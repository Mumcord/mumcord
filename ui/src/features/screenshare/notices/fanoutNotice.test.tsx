/**
 * The relay fan-out notice under the sharer's live bar (FR-7.47, FR-7.49, mockup 06) and the
 * viewer limit of the running share, with screen-reader announcements and an axe check.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, ScreenShare, ShareQuality } from "../../../api";
import { subscribeAnnouncements, type Announcement } from "../../../a11y/announcer";
import { axeViolations } from "../../../test/axe";
import { connectedMock, resetAll, stubDialog } from "../../channels/testUtils";
import { SharerBar } from "../SharerBar";
import { resetScreenShareStore } from "../sharesStore";
import { resetFanoutNotices } from "./fanoutDismissed";

const SELF = 1;
const ANNA = 2;
const NO_RELAY = "This server has no relay for large shares";

function quality(patch: Partial<ShareQuality> = {}): ShareQuality {
  return {
    quality: "1080p30",
    lowered: null,
    switched: null,
    width: 1920,
    height: 1080,
    fps: 30,
    kbps: 4000,
    encoder: { codec: "av1", gpu: "nvidia" },
    viewers: [],
    choice: { resolution: "1440", fps: "60", optimize: "balanced", uploadLimit: "auto" },
    fanout: { kind: "direct", reason: "noRelay" },
    ...patch,
  };
}

const viewer = (session: number): ShareQuality["viewers"][number] => ({
  session,
  fps: 30,
  struggling: false,
  slow: false,
  codec: "av1",
  path: "direct",
});

describe("relay fan-out notice", () => {
  let api: MockApi;
  let heard: Announcement[];
  let unsubscribe: () => void;

  beforeEach(async () => {
    resetScreenShareStore();
    resetFanoutNotices();
    stubDialog();
    api = await connectedMock({ bindStore: true });
    api.mock.emit("voiceServerChanged", { serverId: 1 });
    heard = [];
    unsubscribe = subscribeAnnouncements((announcement) => {
      heard.push(announcement);
    });
  });

  afterEach(() => {
    unsubscribe();
    vi.restoreAllMocks();
    resetScreenShareStore();
    resetAll();
  });

  async function sharing(q: ShareQuality, viewers = [ANNA]): Promise<void> {
    vi.spyOn(api, "screenShareQuality").mockResolvedValue(q);
    render(<SharerBar />);
    await act(async () => {
      await api.screenShareStart(1, false, null, null);
    });
    const share: ScreenShare = {
      session: SELF,
      paused: false,
      sound: false,
      soundAvailable: false,
      viewers,
      blocked: null,
      relayReason: null,
      kind: "window",
      elapsedMs: 0,
    };
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [share] });
    });
  }

  const polite = () => heard.filter((a) => a.politeness === "polite").map((a) => a.message);
  const notice = (name: string | RegExp) => screen.findByRole("region", { name });

  it("explains the copies as mockup 06 does, once, and goes away with Got it", async () => {
    const viewers = [2, 3, 4, 5, 6, 7];
    await sharing(quality({ lowered: "manyViewers", viewers: viewers.map(viewer) }), viewers);
    const card = await notice(NO_RELAY);
    expect(card).toHaveTextContent(
      "Each viewer gets their own copy from your computer, so up to 8 people can watch. " +
        "Quality goes down as more join: right now 6 viewers need about 24 Mbit/s of upload, " +
        "so your share runs at 1080p · 30 fps instead of 1440p · 60 fps.",
    );
    expect(await axeViolations(card)).toEqual([]);
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1100));
    });
    expect(polite().filter((m) => m === NO_RELAY)).toHaveLength(1);

    fireEvent.click(within(card).getByRole("button", { name: "Got it" }));
    expect(screen.queryByRole("region", { name: NO_RELAY })).toBeNull();
  });

  it("stays dismissed for the same share when the bar mounts again", async () => {
    await sharing(quality());
    fireEvent.click(within(await notice(NO_RELAY)).getByRole("button", { name: "Got it" }));
    render(<SharerBar />);
    await waitFor(() => {
      expect(screen.getAllByRole("group", { name: "Your screen share" })).toHaveLength(2);
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
    });
    expect(screen.queryByRole("region", { name: NO_RELAY })).toBeNull();
  });

  it("names the reason when the relay exists but fan-out does not work", async () => {
    await sharing(quality({ fanout: { kind: "direct", reason: "full" } }));
    const text =
      "Relay fan-out isn't available (the relay is full). Each viewer gets their own copy, " +
      "so up to 8 can watch and quality may drop.";
    const card = await notice(text);
    expect(card).toHaveTextContent(text);
    expect(polite()).toContain(text);
  });

  it("without detail, the no-relay card still says what it means", async () => {
    await sharing(quality());
    const card = await notice(NO_RELAY);
    expect(card).toHaveTextContent("Relay fan-out isn't available (this server has no relay).");
  });

  it("shows nothing while the relay carries the share or the share asks for it", async () => {
    await sharing(quality({ fanout: { kind: "starting" } }));
    await screen.findByText("1080p · 30 fps");
    expect(screen.queryByRole("region")).toBeNull();
  });
});

describe("live viewer limit", () => {
  beforeEach(() => {
    resetScreenShareStore();
    resetFanoutNotices();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    resetScreenShareStore();
    resetAll();
  });

  async function popoverWith(fanout: ShareQuality["fanout"]): Promise<HTMLElement> {
    const api = await connectedMock({ bindStore: true });
    api.mock.emit("voiceServerChanged", { serverId: 1 });
    vi.spyOn(api, "screenShareQuality").mockResolvedValue(quality({ fanout }));
    render(<SharerBar />);
    await act(async () => {
      await api.screenShareStart(1, false, null, null);
    });
    act(() => {
      api.mock.emit("screenSharesChanged", {
        serverId: 1,
        shares: [
          {
            session: SELF,
            paused: false,
            sound: false,
            soundAvailable: false,
            viewers: [ANNA],
            blocked: null,
            relayReason: null,
            kind: "window",
            elapsedMs: 0,
          },
        ],
      });
    });
    await screen.findByText("1080p · 30 fps");
    fireEvent.click(screen.getByRole("button", { name: /1 viewer/ }));
    return screen.getByRole("group", { name: "Viewers" });
  }

  it("takes the relay's limit for a broadcast", async () => {
    const popover = await popoverWith({ kind: "relay", limit: 20 });
    expect(popover).toHaveTextContent("1 of 20");
    expect(await axeViolations(popover)).toEqual([]);
  });

  it("takes eight for direct copies", async () => {
    const popover = await popoverWith({ kind: "direct", reason: "lost" });
    expect(popover).toHaveTextContent("1 of 8");
  });
});
