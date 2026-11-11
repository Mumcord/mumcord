/**
 * Camera entry points (FR-8.4, FR-8.9, FR-8.16–8.19): the user-panel button and the Apps-menu
 * entry follow availability and the own camera, show the reason when blocked, open the preview
 * when off, turn the camera off at once when on air, and announce on-air changes.
 */
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { subscribeAnnouncements } from "../../a11y/announcer";
import type { CameraState, MockApi } from "../../api";
import { axeViolations } from "../../test/axe";
import { connectedMock, resetAll } from "../channels/testUtils";
import { cameraBlock, resetCameraAvailability } from "./availability";
import { CameraButton } from "./CameraButton";
import { CameraMenuItem } from "./CameraMenuItem";
import { CameraTreeIcon, CameraTreeLabel } from "./CameraTreeIcon";
import { resetCameraPreview, useCameraPreviewTarget } from "./previewStore";
import { resetCameraRoomStore } from "./room/roomStore";

function PreviewProbe() {
  const target = useCameraPreviewTarget();
  return (
    <output data-testid="preview">{target === null ? "closed" : String(target.serverId)}</output>
  );
}

async function setup(): Promise<MockApi> {
  const api = await connectedMock({ bindStore: true });
  act(() => {
    api.mock.emit("voiceServerChanged", { serverId: 1 });
  });
  return api;
}

function camera(api: MockApi, state: CameraState): void {
  act(() => {
    api.mock.emit("cameraChanged", {
      view: { serverId: 1, state, error: null, broadcast: null, viewers: [] },
    });
  });
}

beforeEach(() => {
  resetCameraAvailability();
  resetCameraPreview();
  resetCameraRoomStore();
});

afterEach(() => {
  vi.restoreAllMocks();
  resetCameraAvailability();
  resetCameraPreview();
  resetCameraRoomStore();
  resetAll();
});

describe("cameraBlock", () => {
  it("needs the voice channel first, then the server's reason", () => {
    expect(cameraBlock(null, 1, null)).toBe("notInChannel");
    expect(cameraBlock(2, 1, null)).toBe("notInChannel");
    expect(cameraBlock(2, 1, "noRelay")).toBe("notInChannel");
    expect(cameraBlock(1, 1, "noCertificate")).toBe("noCertificate");
    expect(cameraBlock(1, 1, null)).toBeNull();
  });
});

describe("user-panel camera button", () => {
  it("opens the preview when the camera is off and available", async () => {
    await setup();
    render(
      <>
        <CameraButton serverId={1} />
        <PreviewProbe />
      </>,
    );
    const button = screen.getByRole("button", { name: "Turn on camera" });
    expect(button).not.toHaveAttribute("aria-disabled");
    expect(button).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(button);
    expect(screen.getByTestId("preview")).toHaveTextContent("1");
  });

  it.each([
    ["noRelay", "This server has no relay, so camera sharing isn't available."],
    ["relayTooOld", "This server's relay is too old for camera sharing."],
    ["relayDisallows", "This server's relay doesn't allow camera sharing."],
    ["needsRelayMode", "Camera sharing is turned off. Change it in Privacy & Connections."],
    ["noCertificate", "Camera sharing needs a client certificate."],
  ] as const)("is disabled with the %s reason as tooltip", async (reason, text) => {
    const api = await setup();
    render(
      <>
        <CameraButton serverId={1} />
        <PreviewProbe />
      </>,
    );
    act(() => {
      api.mock.emit("cameraAvailabilityChanged", { serverId: 1, reason });
    });
    const button = screen.getByRole("button", { name: "Turn on camera" });
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).toHaveAttribute("data-tooltip", text);
    fireEvent.click(button);
    expect(screen.getByTestId("preview")).toHaveTextContent("closed");
  });

  it("asks to join a voice channel first", async () => {
    await setup();
    render(<CameraButton serverId={null} />);
    expect(screen.getByRole("button", { name: "Turn on camera" })).toHaveAttribute(
      "data-tooltip",
      "Join a voice channel to turn on your camera.",
    );
  });

  it("reads the availability of a server connected before first use", async () => {
    const api = await setup();
    vi.spyOn(api, "cameraUnavailable").mockResolvedValue("relayDisallows");
    render(<CameraButton serverId={1} />);
    expect(await screen.findByRole("button", { name: "Turn on camera" })).toHaveAttribute(
      "data-tooltip",
      "This server's relay doesn't allow camera sharing.",
    );
  });

  it("turns red on air, turns off at once and announces both changes", async () => {
    const api = await setup();
    const turnOff = vi.spyOn(api, "cameraTurnOff");
    const heard: string[] = [];
    const stop = subscribeAnnouncements(({ message }) => heard.push(message));
    render(<CameraButton serverId={1} />);
    // Blocked reasons never stop turning the camera off.
    act(() => {
      api.mock.emit("cameraAvailabilityChanged", { serverId: 1, reason: "noRelay" });
    });
    camera(api, "onAir");
    const button = screen.getByRole("button", { name: "Turn off camera. Your camera is on." });
    expect(button).toHaveAttribute("aria-pressed", "true");
    expect(button).toHaveAttribute("data-onair");
    expect(button).not.toHaveAttribute("aria-disabled");
    fireEvent.click(button);
    expect(turnOff).toHaveBeenCalledWith(1);
    camera(api, "off");
    expect(screen.getByRole("button", { name: "Turn on camera" })).toBeInTheDocument();
    expect(heard).toEqual(["Your camera is on", "Your camera is off"]);
    stop();
  });

  it("has no axe violations", async () => {
    const api = await setup();
    const { container } = render(<CameraButton serverId={1} />);
    act(() => {
      api.mock.emit("cameraAvailabilityChanged", { serverId: 1, reason: "noRelay" });
    });
    expect(await axeViolations(container)).toEqual([]);
  });
});

describe("Apps-menu camera entry", () => {
  function menu(onChosen = () => undefined) {
    return render(
      <>
        <div role="menu" aria-label="Start something">
          <CameraMenuItem serverId={1} channelName="Lobby" onChosen={onChosen} />
        </div>
        <PreviewProbe />
      </>,
    );
  }

  it("explains the preview and opens it", async () => {
    await setup();
    const onChosen = vi.fn();
    menu(onChosen);
    const item = screen.getByRole("menuitem", { name: /Turn on camera/ });
    expect(item).toHaveAccessibleDescription(
      "You see a preview first. Only people in Lobby can see you.",
    );
    fireEvent.click(item);
    expect(onChosen).toHaveBeenCalled();
    expect(screen.getByTestId("preview")).toHaveTextContent("1");
  });

  it("shows the reason under a disabled entry", async () => {
    const api = await setup();
    const onChosen = vi.fn();
    const { container } = menu(onChosen);
    act(() => {
      api.mock.emit("cameraAvailabilityChanged", { serverId: 1, reason: "noRelay" });
    });
    const item = screen.getByRole("menuitem", { name: /Turn on camera/ });
    expect(item).toHaveAttribute("aria-disabled", "true");
    expect(item).toHaveAccessibleDescription(
      "This server has no relay, so camera sharing isn't available.",
    );
    fireEvent.click(item);
    expect(onChosen).not.toHaveBeenCalled();
    expect(screen.getByTestId("preview")).toHaveTextContent("closed");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("reads Turn off camera on air and turns it off", async () => {
    const api = await setup();
    const turnOff = vi.spyOn(api, "cameraTurnOff");
    menu();
    camera(api, "onAir");
    fireEvent.click(screen.getByRole("menuitem", { name: "Turn off camera" }));
    expect(turnOff).toHaveBeenCalledWith(1);
  });
});

describe("preview store", () => {
  it("opens the dialog for a preview started elsewhere (the hotkey)", async () => {
    const api = await setup();
    render(<PreviewProbe />);
    camera(api, "preview");
    expect(screen.getByTestId("preview")).toHaveTextContent("1");
  });
});

describe("channel-tree camera marker", () => {
  it("marks the own row only while on air", async () => {
    const api = await setup();
    const { container } = render(
      <>
        <span data-testid="name">
          Priya
          <CameraTreeLabel self session={1} name="Priya" />
        </span>
        <CameraTreeIcon self session={1} />
        <CameraTreeIcon self={false} session={2} />
      </>,
    );
    expect(container.querySelector(".camera-tree-icon")).toBeNull();
    camera(api, "preview");
    expect(container.querySelector(".camera-tree-icon")).toBeNull();
    camera(api, "onAir");
    expect(container.querySelectorAll(".camera-tree-icon")).toHaveLength(1);
    expect(container.querySelector(".camera-tree-icon")).toHaveAttribute("data-self");
    expect(screen.getByTestId("name")).toHaveTextContent("Priya, Your camera is on");
  });

  it("marks other people whose camera is on, from the camera room (FR-8.18)", async () => {
    const api = await setup();
    const { container } = render(
      <>
        <span data-testid="name">
          Jonas
          <CameraTreeLabel self={false} session={2} name="Jonas" />
        </span>
        <CameraTreeIcon self={false} session={2} />
        <CameraTreeIcon self={false} session={3} />
      </>,
    );
    expect(container.querySelector(".camera-tree-icon")).toBeNull();
    const tile = {
      sharer: 2,
      state: "hidden" as const,
      hidden: false,
      quality: "auto" as const,
      dataSaver: false,
      fpsBadge: null,
    };
    act(() => {
      api.mock.emit("cameraRoomChanged", {
        view: { serverId: 1, open: false, videosHidden: false, tiles: [tile] },
      });
    });
    expect(container.querySelectorAll(".camera-tree-icon")).toHaveLength(1);
    expect(container.querySelector(".camera-tree-icon")).not.toHaveAttribute("data-self");
    expect(screen.getByTestId("name")).toHaveTextContent("Jonas, Jonas's camera is on");
    act(() => {
      api.mock.emit("cameraRoomChanged", {
        view: { serverId: 1, open: false, videosHidden: false, tiles: [] },
      });
    });
    expect(container.querySelector(".camera-tree-icon")).toBeNull();
  });
});
