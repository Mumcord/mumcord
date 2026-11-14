/** The camera room in the main window (spec 08 §4.8, FR-8.53, FR-8.57, FR-8.61–8.69). */
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CameraTile, CameraView, MockApi } from "../../../api";
import { subscribeAnnouncements } from "../../../a11y/announcer";
import { axeViolations } from "../../../test/axe";
import { connectedMock, resetAll } from "../../channels/testUtils";
import { mediaView, openMedia, resetMediaView, selectStage } from "../../media/mediaView";
import { resetPopoutStore } from "../../media/popoutStore";
import { resetLiveBackground } from "../preview/liveBackground";
import { resetCameraAvailability } from "../availability";
import { resetCameraDecode } from "./cameraDecode";
import { CameraMiniBar } from "./CameraMiniBar";
import { CameraRoom } from "./CameraRoom";
import { CameraRoomSync } from "./CameraRoomSync";
import { resetRoomChoices } from "./roomChoices";
import { resetCameraRoomStore } from "./roomStore";
import { resetCantWatch } from "./useCantWatch";
import { resetCameraLook, setCameraLookSettings } from "./useCameraLook";

// jsdom has no Tauri runtime; the frame channels only need an id.
vi.mock("@tauri-apps/api/core", () => {
  let next = 1;
  return {
    Channel: class {
      id = next++;
      onmessage: (message: ArrayBuffer) => void = () => undefined;
    },
  };
});

const ANNA = 2;
const BEN = 3;

function tile(sharer: number, patch: Partial<CameraTile> = {}): CameraTile {
  return {
    sharer,
    state: "playing",
    hidden: false,
    quality: "auto",
    dataSaver: false,
    fpsBadge: null,
    ...patch,
  };
}

function onAir(api: MockApi): void {
  api.mock.setCameraView({
    serverId: 1,
    state: "onAir",
    error: null,
    broadcast: "00112233aabbccdd",
    viewers: [{ session: ANNA, path: "relay" }],
  });
}

describe("camera room", () => {
  let api: MockApi;

  beforeEach(async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    api = await connectedMock({ bindStore: true });
    await api.channelJoin(1, 1);
    resetCameraRoomStore();
    resetCameraDecode();
    resetMediaView();
    resetPopoutStore();
    resetRoomChoices();
    resetLiveBackground();
    resetCameraAvailability();
    resetCantWatch();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    resetCameraLook();
    resetCameraRoomStore();
    resetCantWatch();
    resetAll();
  });

  it("opens in the media area when the engine opens the room", async () => {
    render(<CameraRoomSync serverId={1} />);
    await act(async () => {
      api.mock.setCameraRoom(1, [tile(ANNA)], true);
      await Promise.resolve();
    });
    expect(mediaView().stage).toContain("camera");
    expect(mediaView().cinema).toBe("camera");
  });

  it("shows a room the engine opened before any camera once the first camera is on", async () => {
    render(<CameraRoomSync serverId={1} />);
    await act(async () => {
      api.mock.setCameraRoom(1, [], true);
      await Promise.resolve();
    });
    expect(mediaView().stage).not.toContain("camera");
    await act(async () => {
      api.mock.setCameraRoom(1, [tile(ANNA)], true);
      await Promise.resolve();
    });
    expect(mediaView().cinema).toBe("camera");
  });

  it("keeps the engine's room open while it comes into the media area", async () => {
    // Closing it on the way would leave every camera and join again, which the sender's join
    // limit holds back for 10 s (spec 08 §6.5).
    const setOpen = vi.spyOn(api, "cameraRoomSetOpen");
    render(<CameraRoomSync serverId={1} />);
    await act(async () => {
      api.mock.setCameraRoom(1, [tile(ANNA)], true);
      await Promise.resolve();
    });
    expect(mediaView().cinema).toBe("camera");
    expect(setOpen).not.toHaveBeenCalledWith(1, false);
  });

  it("shows the header, the tiles, the self-view and who has no camera", async () => {
    onAir(api);
    api.mock.setCameraRoom(1, [tile(ANNA, { fpsBadge: 7 })], true);
    const { container } = render(<CameraRoom serverId={1} onTurnOn={null} />);
    const room = await screen.findByRole("region", { name: "Camera room, 2 cameras on" });
    expect(room).toHaveTextContent("2 cameras on · 3 people in Lobby");
    expect(room).toHaveTextContent("Relay · end-to-end encrypted");
    const anna = await screen.findByRole("group", { name: "Anna, camera on" });
    expect(anna).toHaveTextContent("7 fps");
    const self = screen.getByRole("group", { name: "You, camera on, live" });
    expect(self).toHaveTextContent("Live");
    expect(room).toHaveTextContent("Ben is here without a camera");
    expect(screen.getByRole("radio", { name: "Grid" })).toBeChecked();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("names the path each viewer of the own camera gets", async () => {
    api.mock.setCameraView({
      serverId: 1,
      state: "onAir",
      error: null,
      broadcast: "00112233aabbccdd",
      viewers: [{ session: ANNA, path: "direct" }],
    });
    api.mock.setCameraRoom(1, [tile(ANNA)], true);
    render(<CameraRoom serverId={1} onTurnOn={null} />);
    const room = await screen.findByRole("region", { name: /^Camera room/ });
    // Only a direct viewer: the header says so (FR-8.62).
    expect(room).toHaveTextContent("Direct · end-to-end encrypted");
    const button = await screen.findByRole("button", { name: "Viewers (1)" });
    expect(button).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(button);
    const list = screen.getByRole("list", { name: "Viewers" });
    expect(list).toHaveTextContent("Anna");
    expect(list).toHaveTextContent("Direct");
    fireEvent.keyDown(button, { key: "Escape" });
    expect(screen.queryByRole("list", { name: "Viewers" })).toBeNull();
  });

  it("says both when an old relay and a direct stream serve the camera", async () => {
    api.mock.setCameraView({
      serverId: 1,
      state: "onAir",
      error: null,
      broadcast: "00112233aabbccdd",
      viewers: [
        { session: ANNA, path: "relay" },
        { session: BEN, path: "direct" },
      ],
    });
    render(<CameraRoom serverId={1} onTurnOn={null} />);
    const room = await screen.findByRole("region", { name: /^Camera room/ });
    expect(room).toHaveTextContent("Relay and direct · end-to-end encrypted");
    fireEvent.click(await screen.findByRole("button", { name: "Viewers (2)" }));
    const list = screen.getByRole("list", { name: "Viewers" });
    expect(list).toHaveTextContent("Anna");
    expect(list).toHaveTextContent("Relay");
    expect(list).toHaveTextContent("Ben");
    expect(list).toHaveTextContent("Direct");
  });

  it("follows the Mirror and Reduce motion settings", async () => {
    onAir(api);
    api.mock.setCameraRoom(1, [tile(ANNA)], true);
    const { container } = render(<CameraRoom serverId={1} onTurnOn={null} />);
    await screen.findByRole("group", { name: "You, camera on, live" });
    const room = screen.getByRole("region", { name: "Camera room, 2 cameras on" });
    expect(room).not.toHaveAttribute("data-reduce-motion");
    expect(container.querySelector(".cam-video[data-mirror]")).not.toBeNull();
    act(() => {
      setCameraLookSettings({ cameraMirrorSelf: false, cameraReduceMotion: "on" });
    });
    expect(room).toHaveAttribute("data-reduce-motion");
    expect(container.querySelector(".cam-video[data-mirror]")).toBeNull();
  });

  it("says why a tile has no picture", async () => {
    api.mock.setCameraRoom(
      1,
      [tile(ANNA, { state: "full" }), tile(BEN, { state: "failed" })],
      true,
    );
    render(<CameraRoom serverId={1} onTurnOn={null} />);
    expect(await screen.findByText("Too many people are watching Anna")).toBeInTheDocument();
    expect(screen.getByText("Couldn't join Ben's camera")).toBeInTheDocument();
  });

  it("says when a camera needs a newer Mumcord", async () => {
    api.mock.setCameraRoom(1, [tile(ANNA, { state: "needsUpdate" })], true);
    render(<CameraRoom serverId={1} onTurnOn={null} />);
    expect(await screen.findByRole("group", { name: "Anna, camera on" })).toHaveTextContent(
      "This camera needs Mumcord 0.9.0",
    );
  });

  const CANT_WATCH = "You can't see cameras on this server: This server's relay doesn't allow ";

  it("says why it can't show cameras instead of the tiles (FR-8.5)", async () => {
    vi.spyOn(api, "cameraUnavailable").mockResolvedValue("relayDisallows");
    api.mock.setCameraRoom(1, [tile(ANNA)], true);
    const { container } = render(<CameraRoom serverId={1} onTurnOn={null} />);
    const notice = await screen.findByRole("status");
    expect(notice).toHaveTextContent(`${CANT_WATCH}camera sharing.`);
    // Nobody's camera is drawn for a viewer who cannot watch.
    expect(screen.queryByRole("group", { name: "Anna, camera on" })).toBeNull();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows the tiles of a viewer who can watch", async () => {
    api.mock.setCameraRoom(1, [tile(ANNA)], true);
    render(<CameraRoom serverId={1} onTurnOn={null} />);
    expect(await screen.findByRole("group", { name: "Anna, camera on" })).toBeInTheDocument();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("says nothing about watching when nobody in the channel has a camera on", async () => {
    vi.spyOn(api, "cameraUnavailable").mockResolvedValue("relayDisallows");
    api.mock.setCameraRoom(1, [], true);
    render(<CameraRoom serverId={1} onTurnOn={null} />);
    await screen.findByRole("region", { name: /^Camera room/ });
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("says it once per channel visit, and again after rejoining the channel", async () => {
    vi.spyOn(api, "cameraUnavailable").mockResolvedValue("relayDisallows");
    api.mock.setCameraRoom(1, [tile(ANNA)], true);
    const first = render(<CameraRoom serverId={1} onTurnOn={null} />);
    await screen.findByRole("status");
    first.unmount();
    // The room reopened in the same channel: it stays quiet.
    const second = render(<CameraRoom serverId={1} onTurnOn={null} />);
    await screen.findByRole("region", { name: /^Camera room/ });
    expect(screen.queryByRole("status")).toBeNull();
    second.unmount();

    // Leaving the channel and coming back is a new visit.
    await act(async () => {
      await api.channelJoin(1, 2);
      await api.channelJoin(1, 1);
    });
    render(<CameraRoom serverId={1} onTurnOn={null} />);
    expect(await screen.findByRole("status")).toHaveTextContent(CANT_WATCH);
  });

  it("shows a hidden video again", async () => {
    api.mock.setCameraRoom(1, [tile(ANNA, { state: "hidden", hidden: true })], true);
    render(<CameraRoom serverId={1} onTurnOn={null} />);
    const anna = await screen.findByRole("group", { name: "Anna, camera on" });
    expect(anna).toHaveTextContent("Video hidden");
    await act(async () => {
      fireEvent.click(within(anna).getByRole("button", { name: "Show video" }));
      await Promise.resolve();
    });
    expect(anna).not.toHaveTextContent("Video hidden");
  });

  it("hides the self-view for oneself only and turns the camera off", async () => {
    onAir(api);
    api.mock.setCameraRoom(1, [tile(ANNA)], true);
    render(<CameraRoom serverId={1} onTurnOn={null} />);
    await screen.findByRole("group", { name: "You, camera on, live" });
    fireEvent.click(screen.getByRole("button", { name: "Hide self-view" }));
    expect(screen.queryByRole("group", { name: "You, camera on, live" })).toBeNull();
    expect(screen.getByRole("region", { name: "Camera room, 2 cameras on" })).toBeInTheDocument();
    const turnOff = vi.spyOn(api, "cameraTurnOff");
    fireEvent.click(screen.getByRole("button", { name: "Turn off camera. Your camera is on." }));
    expect(turnOff).toHaveBeenCalledWith(1);
  });

  function stopped(api: MockApi, error: CameraView["error"]): void {
    api.mock.setCameraView({ serverId: 1, state: "off", error, broadcast: null, viewers: [] });
  }

  it("says why the camera stopped below the mini bar until dismissed or turned on again", () => {
    render(<CameraMiniBar serverId={1} />);
    act(() => {
      onAir(api);
    });
    act(() => {
      stopped(api, "relayDown");
    });
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Camera stopped: the relay isn't reachable");
    fireEvent.click(within(alert).getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("alert")).toBeNull();

    act(() => {
      onAir(api);
    });
    act(() => {
      stopped(api, "deviceGone");
    });
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Camera stopped: the camera was disconnected.",
    );
    // Turn on camera opens the preview: the notice goes.
    act(() => {
      api.mock.setCameraView({
        serverId: 1,
        state: "preview",
        error: null,
        broadcast: null,
        viewers: [],
      });
    });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("shows no notice when the user turned the camera off", () => {
    render(<CameraMiniBar serverId={1} />);
    act(() => {
      onAir(api);
    });
    act(() => {
      stopped(api, null);
    });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("says why the camera stopped in the room bar", async () => {
    onAir(api);
    api.mock.setCameraRoom(1, [tile(ANNA)], true);
    act(() => {
      openMedia("camera");
    });
    const { container } = render(<CameraRoom serverId={1} onTurnOn={null} />);
    await screen.findByRole("group", { name: "You, camera on, live" });
    act(() => {
      stopped(api, "relayEnded");
    });
    const bar = screen.getByRole("toolbar", { name: "Camera room controls" });
    expect(within(bar).getByRole("alert")).toHaveTextContent(
      "Camera stopped by the server's relay",
    );
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows the notice once: below the mini bar while another item hides the open room", async () => {
    onAir(api);
    api.mock.setCameraRoom(1, [tile(ANNA)], true);
    act(() => {
      openMedia("camera");
      openMedia("plugin:board");
      selectStage("plugin:board");
    });
    render(
      <>
        <CameraMiniBar serverId={1} />
        <CameraRoom serverId={1} onTurnOn={null} />
      </>,
    );
    await screen.findByRole("group", { name: "You, camera on, live" });
    act(() => {
      stopped(api, "relayDown");
    });
    const bar = screen.getByRole("toolbar", { name: "Camera room controls" });
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(within(bar).queryByRole("alert")).toBeNull();

    act(() => {
      selectStage("camera");
    });
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(within(bar).getByRole("alert")).toHaveTextContent(
      "Camera stopped: the relay isn't reachable",
    );
  });

  it("closes to the mini bar and stops the downloads; Open room joins again", async () => {
    const setOpen = vi.spyOn(api, "cameraRoomSetOpen");
    render(
      <>
        <CameraRoomSync serverId={1} />
        <CameraMiniBar serverId={1} />
        <CameraRoom serverId={1} onTurnOn={null} />
      </>,
    );
    await act(async () => {
      api.mock.setCameraRoom(1, [tile(ANNA)], true);
      await Promise.resolve();
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Close room" }));
      await Promise.resolve();
    });
    expect(mediaView().stage).not.toContain("camera");
    expect(setOpen).toHaveBeenLastCalledWith(1, false);
    const bar = screen.getByRole("group", { name: "Camera room, 1 camera on" });
    expect(bar).toHaveTextContent("1 camera on");
    await act(async () => {
      fireEvent.click(within(bar).getByRole("button", { name: "Open room" }));
      await Promise.resolve();
    });
    expect(mediaView().cinema).toBe("camera");
    expect(setOpen).toHaveBeenLastCalledWith(1, true);
  });

  it("announces cameras turning on and off, bundled", async () => {
    vi.useFakeTimers();
    const spoken: string[] = [];
    const stop = subscribeAnnouncements(({ message }) => spoken.push(message));
    api.mock.setCameraRoom(1, [], true);
    render(<CameraRoom serverId={1} onTurnOn={null} />);
    await act(async () => {
      await Promise.resolve();
    });
    act(() => {
      api.mock.setCameraRoom(1, [tile(ANNA)], true);
    });
    act(() => {
      api.mock.setCameraRoom(1, [tile(ANNA), tile(BEN)], true);
    });
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(spoken).toEqual(["Anna and Ben turned on their cameras"]);
    act(() => {
      api.mock.setCameraRoom(1, [tile(BEN)], true);
    });
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(spoken).toContain("Anna turned off their camera");
    stop();
    vi.useRealTimers();
  });

  it("changes the background while on air without saving it", async () => {
    onAir(api);
    api.mock.setCameraRoom(1, [tile(ANNA)], true);
    const setBackground = vi.spyOn(api, "cameraSetBackground");
    const saved = vi.spyOn(api, "settingsUpdate");
    render(<CameraRoom serverId={1} onTurnOn={null} />);
    const button = await screen.findByRole("button", { name: /^Background: / });
    await act(async () => {
      fireEvent.click(button);
      await Promise.resolve();
    });
    const picker = screen.getByRole("dialog", { name: "Change background" });
    await act(async () => {
      fireEvent.click(within(picker).getByRole("radio", { name: "None" }));
      await Promise.resolve();
    });
    expect(setBackground).toHaveBeenCalledWith({ kind: "none" }, expect.any(Number));
    expect(saved).not.toHaveBeenCalled();
    expect(button).toHaveTextContent("Background: None");
    expect(await axeViolations(picker)).toEqual([]);
  });
});
