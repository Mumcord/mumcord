/**
 * The sharer's control bar (issue #163, FR-3.127): before a share it is the entry point, while
 * sharing it names the source and how long the share runs, shows the viewers, and drives pause and
 * stop through the engine. What blocks starting a share (FR-4.3 – FR-4.5) and the sound of the own
 * share (FR-2.42) stay checked here as well, since the bar is where the user sees them.
 */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, ScreenShare } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { ChannelsPanel } from "../channels";
import { MembersPanel } from "../members/MembersPanel";
import { platformOf, shareBlock } from "./availability";
import { SharerBar } from "./SharerBar";
import { resetScreenShareStore } from "./sharesStore";

const SELF = 1;
const ANNA = 2;
const BEN = 3;
const BAR_NAME = "Your screen share";

function share(patch: Partial<ScreenShare> = {}): ScreenShare {
  return {
    session: SELF,
    paused: false,
    sound: false,
    soundAvailable: false,
    viewers: [],
    blocked: null,
    relayReason: null,
    kind: null,
    elapsedMs: 0,
    ...patch,
  };
}

function onOs(api: MockApi, os: string): void {
  const info = api.appInfo.bind(api);
  vi.spyOn(api, "appInfo").mockImplementation(async () => ({ ...(await info()), os }));
}

/** Connected, with voice on the seeded server, on Linux. */
async function linuxMock(): Promise<MockApi> {
  const api = await connectedMock({ bindStore: true });
  onOs(api, "linux x86_64");
  api.mock.emit("voiceServerChanged", { serverId: 1 });
  return api;
}

const shareButton = () => screen.findByRole("button", { name: /^Share screen/ });

beforeEach(() => {
  resetScreenShareStore();
});

afterEach(() => {
  vi.restoreAllMocks();
  resetScreenShareStore();
  resetAll();
});

describe("share availability", () => {
  const linux = platformOf("linux x86_64");

  it("names the first reason that blocks sharing", () => {
    const off = { blocked: "p2pOff", relayReason: null } as const;
    expect(shareBlock({ inChannel: true, p2p: null, platform: linux })).toBeNull();
    expect(shareBlock({ inChannel: true, p2p: null, platform: null })).toBeNull();
    expect(shareBlock({ inChannel: false, p2p: off, platform: linux })).toEqual({
      kind: "notInChannel",
    });
    expect(shareBlock({ inChannel: true, p2p: off, platform: linux })).toEqual({
      kind: "p2p",
      block: off,
    });
    expect(
      shareBlock({ inChannel: true, p2p: null, platform: platformOf("freebsd x86_64") }),
    ).toEqual({ kind: "unsupportedPlatform" });
  });

  it("supports Linux without sound, macOS and Windows with sound", () => {
    expect(platformOf("Linux x86_64")).toEqual({ supported: true, sound: false });
    expect(platformOf("macos aarch64")).toEqual({ supported: true, sound: true });
    expect(platformOf("windows x86_64")).toEqual({ supported: true, sound: true });
  });
});

describe("sound of the own share", () => {
  async function sharing(soundAvailable: boolean): Promise<void> {
    const api = await linuxMock();
    render(<SharerBar />);
    act(() => {
      api.mock.emit("screenSharesChanged", {
        serverId: 1,
        shares: [share({ soundAvailable, sound: soundAvailable })],
      });
    });
    await screen.findByRole("group", { name: BAR_NAME });
  }

  it("disables the sound switch with its reason when the share has no source", async () => {
    await sharing(false);
    const sound = screen.getByRole("button", { name: "Sound" });
    expect(sound).toHaveAttribute("aria-disabled", "true");
    expect(sound).toHaveAccessibleDescription(/Linux shares no sound/);
  });

  it("names the whole screen on systems that can send sound", async () => {
    const api = await connectedMock({ bindStore: true });
    onOs(api, "windows x86_64");
    api.mock.emit("voiceServerChanged", { serverId: 1 });
    render(<SharerBar />);
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [share()] });
    });
    expect(await screen.findByRole("button", { name: "Sound" })).toHaveAccessibleDescription(
      /This share has no sound/,
    );
  });

  it("shows the sound switch and no hint when the share has a source", async () => {
    await sharing(true);
    const sound = screen.getByRole("button", { name: "Sound" });
    expect(sound).toHaveAttribute("aria-pressed", "true");
    expect(sound).not.toHaveAttribute("aria-disabled");
  });
});

describe("share button", () => {
  it("starts a share in the voice channel and shows the controls", async () => {
    const api = await linuxMock();
    const start = vi.spyOn(api, "screenShareStart");
    render(<SharerBar />);
    const button = await shareButton();
    await waitFor(() => {
      expect(button).not.toHaveAttribute("aria-disabled");
    });
    fireEvent.click(button);
    await screen.findByRole("group", { name: BAR_NAME });
    expect(start).toHaveBeenCalledWith(1, false);
    expect(screen.getByRole("button", { name: /0 viewers/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sound" })).toHaveAttribute("aria-disabled", "true");
  });

  it("is disabled with its reason when not in a voice channel", async () => {
    const api = await connectedMock({ bindStore: true });
    onOs(api, "linux x86_64");
    const start = vi.spyOn(api, "screenShareStart");
    render(<SharerBar />);
    const button = await shareButton();
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).toHaveAttribute("data-tooltip", "Join a voice channel to share your screen.");
    expect(button).toHaveAccessibleDescription("Join a voice channel to share your screen.");
    fireEvent.click(button);
    expect(start).not.toHaveBeenCalled();
  });

  it("stays available with direct connections turned off while the relay can be used", async () => {
    const api = await linuxMock();
    await api.settingsUpdate({ ...(await api.settingsGet()), allowDirectConnections: "no" });
    render(<SharerBar />);
    const button = await shareButton();
    await waitFor(() => {
      expect(button).not.toHaveAttribute("aria-disabled");
    });
  });

  it("is disabled when direct connections are turned off and there is no relay", async () => {
    const api = await linuxMock();
    await api.relayForget(1);
    await api.settingsUpdate({ ...(await api.settingsGet()), allowDirectConnections: "no" });
    render(<SharerBar />);
    const button = await shareButton();
    await waitFor(() => {
      expect(button).toHaveAccessibleDescription(/needs direct connections/);
    });
    expect(button).toHaveAttribute("aria-disabled", "true");
  });

  it("is disabled when peer-to-peer is off", async () => {
    const api = await linuxMock();
    await api.settingsUpdate({ ...(await api.settingsGet()), p2pMode: "off" });
    const start = vi.spyOn(api, "screenShareStart");
    render(<SharerBar />);
    const button = await shareButton();
    await waitFor(() => {
      expect(button).toHaveAccessibleDescription("Peer-to-peer is off.");
    });
    fireEvent.click(button);
    expect(start).not.toHaveBeenCalled();
  });

  it("names the relay reason in relay-only mode without a relay", async () => {
    const api = await linuxMock();
    await api.relayForget(1);
    await api.serverSetP2pModeOverride(1, "relayOnly");
    render(<SharerBar />);
    const button = await shareButton();
    await waitFor(() => {
      expect(button).toHaveAccessibleDescription(/no relay/i);
    });
    expect(button).toHaveAttribute("aria-disabled", "true");
  });

  it("notices peer-to-peer turned off since the last read when clicked", async () => {
    const api = await linuxMock();
    const start = vi.spyOn(api, "screenShareStart");
    render(<SharerBar />);
    const button = await shareButton();
    await waitFor(() => {
      expect(button).not.toHaveAttribute("aria-disabled");
    });
    await api.settingsUpdate({ ...(await api.settingsGet()), p2pMode: "off" });
    fireEvent.click(button);
    await waitFor(() => {
      expect(button).toHaveAttribute("aria-disabled", "true");
    });
    expect(start).not.toHaveBeenCalled();
  });

  it("is disabled on a system without capture", async () => {
    const api = await linuxMock();
    vi.mocked(api.appInfo).mockRestore();
    onOs(api, "freebsd x86_64");
    render(<SharerBar />);
    const button = await shareButton();
    await waitFor(() => {
      expect(button).toHaveAccessibleDescription(
        "Screen sharing isn't available on this system yet.",
      );
    });
  });

  it("treats a closed picker as no error", async () => {
    const api = await linuxMock();
    let finish: (value: null) => void = () => undefined;
    vi.spyOn(api, "screenShareStart").mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const error = vi.spyOn(console, "error");
    render(<SharerBar />);
    const button = await shareButton();
    await waitFor(() => {
      expect(button).not.toHaveAttribute("aria-disabled");
    });
    fireEvent.click(button);
    await screen.findByRole("button", { name: /Choosing what to share/ });
    await act(async () => {
      finish(null);
      await Promise.resolve();
    });
    expect(await shareButton()).not.toHaveAttribute("aria-disabled");
    expect(screen.queryByRole("alert")).toBeNull();
    expect(error).not.toHaveBeenCalled();
  });

  it("reports a failed start", async () => {
    const api = await linuxMock();
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    api.mock.failNext("screenShareStart", "internal", "capture failed");
    render(<SharerBar />);
    const button = await shareButton();
    await waitFor(() => {
      expect(button).not.toHaveAttribute("aria-disabled");
    });
    fireEvent.click(button);
    expect(await screen.findByRole("alert")).toHaveTextContent("Screen sharing could not start.");
  });

  it("points to System Settings when macOS does not allow screen recording", async () => {
    const api = await linuxMock();
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    api.mock.failNext("screenShareStart", "permissionDenied", "screenRecording");
    render(<SharerBar />);
    const button = await shareButton();
    await waitFor(() => {
      expect(button).not.toHaveAttribute("aria-disabled");
    });
    fireEvent.click(button);
    expect(await screen.findByRole("alert")).toHaveTextContent("Allow it in System Settings");
  });
});

describe("sharer controls", () => {
  it("pauses, resumes and stops the own share", async () => {
    const api = await linuxMock();
    render(<SharerBar />);
    await shareButton();
    await act(async () => {
      await api.screenShareStart(1, false);
    });
    fireEvent.click(await screen.findByRole("button", { name: "Pause" }));
    expect(await screen.findByText(/Paused/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Resume" }));
    await screen.findByRole("button", { name: "Pause" });
    fireEvent.click(screen.getByRole("button", { name: "Stop sharing" }));
    expect(await shareButton()).toBeInTheDocument();
  });

  it("lists who is watching and says when the share is full", async () => {
    const api = await linuxMock();
    render(<SharerBar />);
    await shareButton();
    act(() => {
      api.mock.emit("screenSharesChanged", {
        serverId: 1,
        shares: [share({ viewers: [ANNA, BEN] })],
      });
    });
    fireEvent.click(await screen.findByRole("button", { name: /2 viewers/ }));
    const popover = screen.getByRole("group", { name: "Viewers" });
    expect(popover).toHaveTextContent("Anna");
    expect(popover).toHaveTextContent("Ben");
    expect(popover).toHaveTextContent("2 of 8");
    act(() => {
      api.mock.emit("screenSharesChanged", {
        serverId: 1,
        shares: [share({ viewers: [ANNA, BEN, 10, 11, 12, 13, 14, 15] })],
      });
    });
    expect(popover).toHaveTextContent("8 of 8");
    expect(popover).toHaveTextContent("All places are taken.");
  });
});

describe("Live marker", () => {
  it("marks a sharing person in the member list until the share ends", async () => {
    const api = await linuxMock();
    render(<MembersPanel />);
    await screen.findByRole("heading", { name: "In Lobby (4)" });
    expect(screen.queryByRole("img", { name: "sharing their screen" })).toBeNull();
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [share({ session: ANNA })] });
    });
    expect(
      await screen.findByRole("button", { name: /^Anna sharing their screen/ }),
    ).toBeInTheDocument();
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [] });
    });
    await waitFor(() => {
      expect(screen.queryByRole("img", { name: "sharing their screen" })).toBeNull();
    });
  });

  it("marks a sharing person in the channel tree", async () => {
    const api = await linuxMock();
    render(<ChannelsPanel />);
    await screen.findByRole("tree");
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [share({ session: ANNA })] });
    });
    expect(await screen.findByRole("img", { name: "sharing their screen" })).toBeInTheDocument();
  });

  it("drops the marker when the connection ends", async () => {
    const api = await linuxMock();
    render(<MembersPanel />);
    await screen.findByRole("heading", { name: "In Lobby (4)" });
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [share({ session: ANNA })] });
    });
    await screen.findByRole("img", { name: "sharing their screen" });
    await act(async () => {
      await api.disconnect(1);
    });
    expect(screen.queryByRole("img", { name: "sharing their screen" })).toBeNull();
  });
});

describe("sharer control bar", () => {
  let api: MockApi;

  beforeEach(async () => {
    api = await linuxMock();
  });

  /** The own share with two viewers, as the engine reports it. */
  function watchedBy(own: ScreenShare): void {
    api.mock.emit("screenSharesChanged", { serverId: 1, shares: [own] });
  }

  it("is the entry point while nothing is shared", () => {
    render(<SharerBar />);
    expect(screen.getByRole("button", { name: /^Share screen/ })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: BAR_NAME })).not.toHaveTextContent("Live");
  });

  it("shows the source, the running time and that it is live", async () => {
    render(<SharerBar />);
    await api.screenShareStart(1, false);
    const bar = screen.getByRole("group", { name: BAR_NAME });
    await vi.waitFor(() => {
      expect(bar).toHaveTextContent("Live");
      expect(bar).toHaveTextContent("You are sharing0:00");
      expect(bar).toHaveTextContent("Shared picture");
      expect(bar).toHaveTextContent("no sound");
    });
  });

  it("counts the viewers, and the viewers button names them with their quality", async () => {
    vi.spyOn(api, "screenShareQuality").mockResolvedValue({
      width: 1920,
      height: 1080,
      fps: 60,
      kbps: 4230,
      viewers: [
        { session: ANNA, fps: 60, struggling: false },
        { session: BEN, fps: 20, struggling: true },
      ],
    });
    render(<SharerBar />);
    await api.screenShareStart(1, false);
    const bar = screen.getByRole("group", { name: BAR_NAME });
    await vi.waitFor(() => expect(bar).toHaveTextContent("1080p · 60 fps"));
    act(() => {
      watchedBy(share({ viewers: [ANNA, BEN], kind: "window", elapsedMs: 60000 }));
    });
    expect(bar).toHaveTextContent("1:00");
    fireEvent.click(screen.getByRole("button", { name: /2 viewers/ }));
    const popover = screen.getByRole("group", { name: "Viewers" });
    await vi.waitFor(() => {
      expect(popover).toHaveTextContent("1080p · 60 fps");
      expect(popover).toHaveTextContent("1080p · 20 fps");
      expect(popover).toHaveTextContent("Quality adapts to the slowest viewer");
    });
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("group", { name: "Viewers" })).toBeNull();
  });

  it("shows the live picture of what the user shares", async () => {
    render(<SharerBar />);
    await api.screenShareStart(1, false);
    await vi.waitFor(() => {
      const picture = screen.getByRole("img", { name: "Preview of what you share" });
      expect(picture).toHaveAttribute("src", expect.stringMatching(/^data:image\/webp;base64,/));
    });
  });

  it("pause reaches the engine and shows the share as paused", async () => {
    render(<SharerBar />);
    await api.screenShareStart(1, false);
    const call = vi.spyOn(api, "screenShareSetPaused");
    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    expect(call).toHaveBeenCalledWith(1, true);
    await vi.waitFor(() =>
      expect(screen.getByRole("group", { name: BAR_NAME })).toHaveTextContent("Paused"),
    );
  });

  it("stop ends the share and the bar is the entry point again", async () => {
    render(<SharerBar />);
    await api.screenShareStart(1, false);
    fireEvent.click(screen.getByRole("button", { name: "Stop sharing" }));
    await vi.waitFor(() =>
      expect(screen.getByRole("button", { name: /^Share screen/ })).toBeInTheDocument(),
    );
  });
});
