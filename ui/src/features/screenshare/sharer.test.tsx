import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, ScreenShare } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { ChannelsPanel } from "../channels";
import { MembersPanel } from "../members/MembersPanel";
import { platformOf, shareBlock } from "./availability";
import { SharePanel } from "./SharePanel";
import { SharingBar } from "./SharingBar";
import { resetScreenShareStore } from "./sharesStore";

const SELF = 1;
const ANNA = 2;
const BEN = 3;

function share(patch: Partial<ScreenShare> = {}): ScreenShare {
  return {
    session: SELF,
    paused: false,
    sound: false,
    soundAvailable: false,
    viewers: [],
    blocked: null,
    relayReason: null,
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
    expect(shareBlock({ inChannel: true, consent: "yes", platform: linux })).toBeNull();
    expect(shareBlock({ inChannel: true, consent: "unset", platform: null })).toBeNull();
    expect(shareBlock({ inChannel: false, consent: "no", platform: linux })).toBe("notInChannel");
    expect(shareBlock({ inChannel: true, consent: "no", platform: linux })).toBe("consentOff");
    expect(
      shareBlock({ inChannel: true, consent: "yes", platform: platformOf("freebsd x86_64") }),
    ).toBe("unsupportedPlatform");
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
    render(<SharePanel />);
    act(() => {
      api.mock.emit("screenSharesChanged", {
        serverId: 1,
        shares: [share({ soundAvailable, sound: soundAvailable })],
      });
    });
    await screen.findByRole("region", { name: "Your screen share" });
  }

  it("hints and hides the sound switch when the share has no source", async () => {
    await sharing(false);
    expect(screen.getByText(/Linux shares no sound/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Share sound" })).toBeNull();
  });

  it("names the whole screen on systems that can send sound", async () => {
    const api = await connectedMock({ bindStore: true });
    onOs(api, "windows x86_64");
    api.mock.emit("voiceServerChanged", { serverId: 1 });
    render(<SharePanel />);
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [share()] });
    });
    expect(await screen.findByText(/This share has no sound/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Share sound" })).toBeNull();
  });

  it("shows the sound switch and no hint when the share has a source", async () => {
    await sharing(true);
    expect(screen.getByRole("button", { name: "Share sound" })).toBeInTheDocument();
    expect(screen.queryByText(/no sound/)).toBeNull();
  });
});

describe("share button", () => {
  it("starts a share in the voice channel and shows the controls", async () => {
    const api = await linuxMock();
    const start = vi.spyOn(api, "screenShareStart");
    render(<SharePanel />);
    const button = await shareButton();
    await waitFor(() => {
      expect(button).not.toHaveAttribute("aria-disabled");
    });
    fireEvent.click(button);
    await screen.findByRole("region", { name: "Your screen share" });
    expect(start).toHaveBeenCalledWith(1, false);
    expect(screen.getByText("Nobody is watching yet")).toBeInTheDocument();
    expect(screen.getByText(/Linux shares no sound/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Share sound" })).toBeNull();
  });

  it("is disabled with its reason when not in a voice channel", async () => {
    const api = await connectedMock({ bindStore: true });
    onOs(api, "linux x86_64");
    const start = vi.spyOn(api, "screenShareStart");
    render(<SharePanel />);
    const button = await shareButton();
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).toHaveAttribute("data-tooltip", "Join a voice channel to share your screen.");
    expect(button).toHaveAccessibleDescription("Join a voice channel to share your screen.");
    fireEvent.click(button);
    expect(start).not.toHaveBeenCalled();
  });

  it("is disabled when direct connections are turned off", async () => {
    const api = await linuxMock();
    await api.settingsUpdate({ ...(await api.settingsGet()), allowDirectConnections: "no" });
    render(<SharePanel />);
    const button = await shareButton();
    await waitFor(() => {
      expect(button).toHaveAccessibleDescription(/needs direct connections/);
    });
    expect(button).toHaveAttribute("aria-disabled", "true");
  });

  it("notices consent turned off since the last read when clicked", async () => {
    const api = await linuxMock();
    const start = vi.spyOn(api, "screenShareStart");
    render(<SharePanel />);
    const button = await shareButton();
    await waitFor(() => {
      expect(button).not.toHaveAttribute("aria-disabled");
    });
    await api.settingsUpdate({ ...(await api.settingsGet()), allowDirectConnections: "no" });
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
    render(<SharePanel />);
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
    render(<SharePanel />);
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
    render(<SharePanel />);
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
    render(<SharePanel />);
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
    render(<SharePanel />);
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
    render(<SharePanel />);
    await shareButton();
    act(() => {
      api.mock.emit("screenSharesChanged", {
        serverId: 1,
        shares: [share({ viewers: [ANNA, BEN] })],
      });
    });
    expect(await screen.findByText("2 watching: Anna, Ben")).toBeInTheDocument();
    act(() => {
      api.mock.emit("screenSharesChanged", {
        serverId: 1,
        shares: [share({ viewers: [ANNA, BEN, 10, 11, 12, 13, 14, 15] })],
      });
    });
    expect(await screen.findByText(/^Share full: 8 of 8 watching: Anna, Ben/)).toBeInTheDocument();
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

describe("Sharing bar and live ring", () => {
  const sharing = (api: MockApi, viewers: number[] = []) => {
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [share({ viewers })] });
    });
  };

  it("shows the bar only while sharing, with viewers, and End stops the share", async () => {
    const api = await linuxMock();
    const stop = vi.spyOn(api, "screenShareStop").mockResolvedValue(null);
    render(<SharingBar />);
    expect(screen.queryByRole("status")).toBeNull();
    sharing(api, [ANNA, BEN]);
    const bar = await screen.findByRole("status");
    expect(bar).toHaveTextContent("You are sharing your screen");
    expect(bar).toHaveTextContent("Viewers: 2");
    fireEvent.click(screen.getByRole("button", { name: "End screen share" }));
    expect(stop).toHaveBeenCalledWith(1);
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [] });
    });
    await waitFor(() => {
      expect(screen.queryByRole("status")).toBeNull();
    });
  });

  it("rings the avatar of a sharer in the tree and the member list", async () => {
    const api = await linuxMock();
    const { container } = render(
      <>
        <ChannelsPanel />
        <MembersPanel />
      </>,
    );
    await screen.findByRole("tree");
    expect(container.querySelector(".avatar[data-live]")).toBeNull();
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [share({ session: ANNA })] });
    });
    await waitFor(() => {
      expect(container.querySelectorAll(".avatar[data-live]").length).toBeGreaterThanOrEqual(2);
    });
    expect(screen.getAllByRole("img", { name: "sharing their screen" }).length).toBeGreaterThan(0);
  });
});
