/** The tile menu and the tile keys in the camera room (spec 08 §5.3, §5.5, FR-8.56, FR-8.57). */
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CameraTile, MockApi } from "../../../api";
import { axeViolations } from "../../../test/axe";
import { connectedMock, resetAll } from "../../channels/testUtils";
import { resetMediaView } from "../../media/mediaView";
import { resetPopoutStore } from "../../media/popoutStore";
import { resetCameraDecode } from "./cameraDecode";
import { CameraRoom } from "./CameraRoom";
import { resetRoomChoices, useRoomChoices, type RoomChoices } from "./roomChoices";
import { resetCameraRoomStore } from "./roomStore";
import { closeTileMenu } from "./TileMenuActions";

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

/** Shows the room choices, so the test can read what a key changed. */
function ChoicesProbe() {
  return <output data-testid="choices">{JSON.stringify(useRoomChoices())}</output>;
}

function choices(): RoomChoices {
  return JSON.parse(screen.getByTestId("choices").textContent) as RoomChoices;
}

describe("tile menu", () => {
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
    closeTileMenu();
    api.mock.setCameraRoom(1, [tile(ANNA), tile(BEN)], true);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    resetCameraRoomStore();
    resetAll();
  });

  async function renderRoom() {
    const result = render(
      <>
        <CameraRoom serverId={1} onTurnOn={null} />
        <ChoicesProbe />
      </>,
    );
    await screen.findByRole("group", { name: "Anna, camera on" });
    return result;
  }

  it("keeps the tiles one Tab stop and moves with the arrow keys", async () => {
    await renderRoom();
    const anna = screen.getByRole("group", { name: "Anna, camera on" });
    const ben = screen.getByRole("group", { name: "Ben, camera on" });
    expect(anna).toHaveAttribute("tabindex", "0");
    expect(ben).toHaveAttribute("tabindex", "-1");
    act(() => {
      anna.focus();
    });
    fireEvent.keyDown(anna, { key: "ArrowRight" });
    expect(ben).toHaveFocus();
    expect(ben).toHaveAttribute("tabindex", "0");
    expect(anna).toHaveAttribute("tabindex", "-1");
    fireEvent.keyDown(ben, { key: "Home" });
    expect(anna).toHaveFocus();
  });

  it("opens the menu with Enter and offers the viewer choices", async () => {
    const { container } = await renderRoom();
    const anna = screen.getByRole("group", { name: "Anna, camera on" });
    act(() => {
      anna.focus();
    });
    fireEvent.keyDown(anna, { key: "Enter" });
    const menu = screen.getByRole("menu", { name: "Options for Anna" });
    expect(within(menu).getByRole("menuitem", { name: /Pin for me/ })).toHaveFocus();
    expect(within(menu).getByRole("menuitem", { name: /Show large/ })).toBeInTheDocument();
    expect(within(menu).getByRole("menuitem", { name: /Hide Anna's video/ })).toBeInTheDocument();
    expect(within(menu).getByRole("menuitemradio", { name: /Auto/ })).toBeChecked();
    expect(menu).toHaveTextContent("Hidden videos aren't downloaded at all.");
    expect(await axeViolations(container)).toEqual([]);

    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(anna).toHaveFocus();
  });

  it("sets the quality for me through the engine", async () => {
    const setQuality = vi.spyOn(api, "cameraTileSetQuality");
    await renderRoom();
    fireEvent.click(screen.getByRole("button", { name: "Options for Ben" }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: /Data saver/ }));
    expect(setQuality).toHaveBeenCalledWith(1, BEN, "dataSaver");
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("pins, shows large and hides with the tile keys", async () => {
    const setHidden = vi.spyOn(api, "cameraTileSetHidden");
    await renderRoom();
    const ben = screen.getByRole("group", { name: "Ben, camera on" });
    act(() => {
      ben.focus();
    });
    fireEvent.keyDown(ben, { key: "p" });
    expect(choices().pinned).toEqual([BEN]);
    fireEvent.keyDown(screen.getByRole("group", { name: "Ben, camera on" }), { key: "f" });
    expect(choices().large).toBe(BEN);
    fireEvent.keyDown(screen.getByRole("group", { name: "Ben, camera on" }), { key: "Escape" });
    expect(choices().large).toBeNull();
    fireEvent.keyDown(screen.getByRole("group", { name: "Ben, camera on" }), { key: "h" });
    expect(setHidden).toHaveBeenCalledWith(1, BEN, true);
  });

  it("offers Hide self-view on the own tile instead of hide and quality", async () => {
    api.mock.setCameraView({
      serverId: 1,
      state: "onAir",
      error: null,
      broadcast: "00112233aabbccdd",
      viewers: [{ session: ANNA, path: "relay" }],
    });
    await renderRoom();
    const self = await screen.findByRole("group", { name: "You, camera on, live" });
    act(() => {
      self.focus();
    });
    fireEvent.keyDown(self, { key: "F10", shiftKey: true });
    const menu = screen.getByRole("menu");
    expect(within(menu).queryByRole("menuitemradio")).toBeNull();
    fireEvent.click(within(menu).getByRole("menuitem", { name: /Hide self-view/ }));
    expect(choices().selfHidden).toBe(true);
  });
});
