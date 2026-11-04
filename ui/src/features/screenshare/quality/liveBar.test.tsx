/**
 * The sharer's live bar in spec 07 (FR-7.38, 7.39, 7.57, 7.58; mockups 02 and 07): the quality,
 * codec and path badges, the Quality menu and the notices about lowered quality and encoder
 * switches, with keyboard use, screen-reader announcements and an axe check.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, ScreenShare, Settings, ShareQuality } from "../../../api";
import { subscribeAnnouncements, type Announcement } from "../../../a11y/announcer";
import { axeViolations } from "../../../test/axe";
import { connectedMock, resetAll, stubDialog } from "../../channels/testUtils";
import { SharerBar } from "../SharerBar";
import { resetScreenShareStore } from "../sharesStore";
import { checkedRow, chooseRow, codecBadge, sharePath } from "./qualityModel";

const SELF = 1;
const ANNA = 2;
const BAR_NAME = "Your screen share";

const CHOICE = {
  resolution: "1080",
  fps: "30",
  optimize: "balanced",
  uploadLimit: "auto",
} as const;

function quality(patch: Partial<ShareQuality> = {}): ShareQuality {
  return {
    quality: "1080p30",
    lowered: null,
    switched: null,
    width: 1920,
    height: 1080,
    fps: 30,
    kbps: 4000,
    encoder: { codec: "av1", gpu: null },
    viewers: [],
    ...patch,
  };
}

const LOAD_SWITCH: ShareQuality["switched"] = {
  reason: { kind: "load", details: { encodeMs: 41.6, budgetMs: 33.3, dropped: 12 } },
  from: { codec: "av1", gpu: null },
  to: { codec: "vp9", gpu: null },
  undo: true,
};

describe("live bar model", () => {
  it("finds the checked row of a pick", () => {
    expect(checkedRow({ ...CHOICE, resolution: "auto" })).toBe("auto");
    expect(checkedRow(CHOICE)).toBe(2);
    expect(checkedRow({ ...CHOICE, resolution: "720", fps: "60" })).toBeNull();
  });

  it("keeps Optimize for and the upload limit, and Auto takes the Settings frame rate", () => {
    const pick = { ...CHOICE, optimize: "text", uploadLimit: "5" } as const;
    expect(chooseRow(pick, 0, "30")).toEqual({ ...pick, resolution: "1440", fps: "60" });
    expect(chooseRow(pick, "auto", "15")).toEqual({ ...pick, resolution: "auto", fps: "15" });
  });

  it("names the codec and the path", () => {
    expect(codecBadge(null)).toBeNull();
    expect(codecBadge({ codec: "av1", gpu: "nvidia" })).toEqual({ codec: "AV1", hardware: true });
    expect(codecBadge({ codec: "vp9", gpu: null })).toEqual({ codec: "VP9", hardware: false });
    const viewer = { session: 2, fps: 30, struggling: false, slow: false, codec: null };
    expect(sharePath([])).toBeNull();
    expect(sharePath([{ ...viewer, path: "direct" }])).toBe("direct");
    expect(
      sharePath([
        { ...viewer, path: "direct" },
        { ...viewer, path: "broadcast" },
      ]),
    ).toBe("relay");
  });
});

describe("sharer live bar", () => {
  let api: MockApi;
  let heard: Announcement[];
  let unsubscribe: () => void;

  beforeEach(async () => {
    resetScreenShareStore();
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

  async function sharing(q: ShareQuality, settings: Partial<Settings> = {}): Promise<HTMLElement> {
    vi.spyOn(api, "screenShareQuality").mockResolvedValue(q);
    const loaded = await api.settingsGet();
    vi.spyOn(api, "settingsGet").mockResolvedValue({ ...loaded, ...settings });
    render(<SharerBar />);
    await act(async () => {
      await api.screenShareStart(1, false);
    });
    const share: ScreenShare = {
      session: SELF,
      paused: false,
      sound: false,
      soundAvailable: false,
      viewers: [ANNA],
      blocked: null,
      relayReason: null,
      kind: "window",
      elapsedMs: 0,
    };
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [share] });
    });
    return screen.getByRole("group", { name: BAR_NAME });
  }

  const polite = () => heard.filter((a) => a.politeness === "polite").map((a) => a.message);

  it("shows the quality, codec and path badges", async () => {
    const bar = await sharing(
      quality({
        encoder: { codec: "av1", gpu: "nvidia" },
        viewers: [
          { session: ANNA, fps: 30, struggling: false, slow: false, codec: "av1", path: "relay" },
        ],
      }),
    );
    await waitFor(() => {
      expect(bar).toHaveTextContent("1080p · 30 fps");
      expect(bar).toHaveTextContent("AV1 · hardware");
      expect(bar).toHaveTextContent("Relay");
    });
    expect(within(bar).getByText("AV1 · hardware")).toHaveAttribute("data-tone", "accent");
  });

  it("marks the codec after a switch and offers Undo and Details", async () => {
    const undo = vi.spyOn(api, "screenShareUndoEncoderSwitch").mockResolvedValue(null);
    const bar = await sharing(
      quality({ encoder: { codec: "vp9", gpu: null }, switched: LOAD_SWITCH }),
    );
    await waitFor(() =>
      expect(within(bar).getByText("VP9 · CPU")).toHaveAttribute("data-tone", "warn"),
    );
    const notice = screen.getByRole("region", {
      name: "Your computer couldn't keep up with AV1 — switched to VP9 to keep the share smooth",
    });
    expect(notice).toHaveTextContent("Your share stays at 1080p · 30 fps with VP9");
    expect(polite()).toEqual([
      "Your computer couldn't keep up with AV1 — switched to VP9 to keep the share smooth",
    ]);
    fireEvent.click(within(notice).getByRole("button", { name: "Undo" }));
    expect(undo).toHaveBeenCalledWith(1);

    fireEvent.click(within(notice).getByRole("button", { name: "Details" }));
    const dialog = screen.getByRole("dialog", { name: "Why your share switched encoders" });
    expect(dialog).toHaveTextContent("AV1 → VP9");
    expect(dialog).toHaveTextContent("42 ms");
    expect(dialog).toHaveTextContent("33 ms");
    expect(dialog).toHaveTextContent("Dropped frames12");
    expect(await axeViolations(dialog)).toEqual([]);
    const close = within(dialog).getByRole("button", { name: "Close" });
    expect(close).toHaveFocus();
    fireEvent.click(close);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(await axeViolations(notice)).toEqual([]);
  });

  it("hides Undo after a second fallback in the share", async () => {
    await sharing(quality({ switched: { ...LOAD_SWITCH, undo: false } }));
    const notice = await screen.findByRole("region");
    expect(within(notice).queryByRole("button", { name: "Undo" })).toBeNull();
    expect(within(notice).getByRole("button", { name: "Details" })).toBeInTheDocument();
  });

  it("says once why the quality is lowered", async () => {
    const bar = await sharing(quality({ height: 720, fps: 30, lowered: "uploadTooSlow" }));
    await waitFor(() => expect(bar).toHaveTextContent("Lowered to 720p · 30 fps: upload too slow"));
    expect(within(bar).getByText("720p · 30 fps")).toHaveAttribute("data-tone", "warn");
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1100));
    });
    expect(polite()).toEqual(["Lowered to 720p · 30 fps: upload too slow"]);
  });

  it("notes the switch to VP9 for a viewer who cannot play AV1", async () => {
    const bar = await sharing(
      quality({
        encoder: { codec: "vp9", gpu: "intel" },
        switched: {
          reason: { kind: "viewer", session: ANNA },
          from: { codec: "av1", gpu: "intel" },
          to: { codec: "vp9", gpu: "intel" },
          undo: false,
        },
      }),
    );
    await waitFor(() => expect(bar).toHaveTextContent("Switched to VP9 for Anna"));
    expect(screen.queryByRole("region")).toBeNull();
    expect(polite()).toContain("Switched to VP9 for Anna");
  });

  it("changes the quality from the menu with the keyboard", async () => {
    const set = vi.spyOn(api, "screenShareSetQuality");
    const bar = await sharing(quality(), { screenResolution: "1080", screenFps: "30" });
    const button = within(bar).getByRole("button", { name: "Quality" });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    const menu = screen.getByRole("menu", { name: "Quality" });
    const checked = within(menu).getByRole("menuitemradio", { name: /1080p · 30 fps/ });
    expect(checked).toHaveAttribute("aria-checked", "true");
    expect(checked).toHaveFocus();
    expect(
      screen.getByText("Changes apply within a second, without restarting the share."),
    ).toBeVisible();
    expect(await axeViolations(bar.parentElement ?? bar)).toEqual([]);

    fireEvent.keyDown(checked, { key: "ArrowUp" });
    const best = within(menu).getByRole("menuitemradio", { name: /1080p · 60 fps/ });
    expect(best).toHaveFocus();
    fireEvent.keyDown(best, { key: "End" });
    expect(within(menu).getByRole("menuitemradio", { name: "Smooth motion" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement ?? menu, { key: "Home" });
    expect(within(menu).getByRole("menuitemradio", { name: /Auto/ })).toHaveFocus();
    fireEvent.keyDown(document.activeElement ?? menu, { key: "ArrowDown" });
    fireEvent.click(document.activeElement ?? menu);
    expect(set).toHaveBeenCalledWith(1, {
      resolution: "1440",
      fps: "60",
      optimize: "balanced",
      uploadLimit: "auto",
    });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(button).toHaveFocus();

    fireEvent.click(button);
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Sharp text" }));
    expect(set).toHaveBeenLastCalledWith(1, {
      resolution: "1440",
      fps: "60",
      optimize: "text",
      uploadLimit: "auto",
    });

    fireEvent.click(button);
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(button).toHaveFocus();
  });

  it("Auto keeps the Settings frame rate, and a refused change is reported", async () => {
    const set = vi
      .spyOn(api, "screenShareSetQuality")
      .mockRejectedValue({ code: "invalid", message: "no" });
    const bar = await sharing(quality(), { screenResolution: "720", screenFps: "15" });
    const button = within(bar).getByRole("button", { name: "Quality" });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    expect(
      within(screen.getByRole("menu")).queryAllByRole("menuitemradio", { checked: true }),
    ).toEqual([]);
    fireEvent.click(screen.getByRole("menuitemradio", { name: /Auto/ }));
    expect(set).toHaveBeenCalledWith(1, {
      resolution: "auto",
      fps: "15",
      optimize: "balanced",
      uploadLimit: "auto",
    });
    await waitFor(() => {
      expect(heard).toContainEqual({
        message: "The screen share did not take your change.",
        politeness: "assertive",
      });
    });
  });
});
