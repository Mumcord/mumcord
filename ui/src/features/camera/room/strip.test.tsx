/** The camera strip next to a screen share or watch party (spec 08 §5.4, FR-8.71–8.74). */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CameraTile, MockApi } from "../../../api";
import { axeViolations } from "../../../test/axe";
import { connectedMock, resetAll } from "../../channels/testUtils";
import { closeMedia, openMedia, resetMediaView, selectStage } from "../../media/mediaView";
import { resetPopoutStore } from "../../media/popoutStore";
import { resetCameraDecode } from "./cameraDecode";
import { resetRoomChoices } from "./roomChoices";
import { resetCameraRoomStore } from "./roomStore";
import { StripLayout } from "./StripLayout";
import {
  leaveLarge,
  resetStripState,
  setStripPosition,
  showLarge,
  useStripState,
} from "./StripState";

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

function tile(sharer: number): CameraTile {
  return {
    sharer,
    state: "playing",
    hidden: false,
    quality: "auto",
    dataSaver: false,
    fpsBadge: null,
  };
}

/** Stands in for the share view: counts its mounts, so a remount would show. */
let mounts = 0;
function FakeShare() {
  return (
    <div
      ref={(element) => {
        if (element !== null) mounts += 1;
      }}
      data-testid="share"
    />
  );
}

function Position() {
  const { position, large } = useStripState();
  return <output data-testid="state">{`${position} ${String(large)}`}</output>;
}

describe("camera strip", () => {
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
    resetStripState();
    mounts = 0;
  });

  afterEach(() => {
    vi.restoreAllMocks();
    resetCameraRoomStore();
    resetMediaView();
    resetStripState();
    resetAll();
  });

  /** Cameras on and a watched share as the main item. */
  function combined(): void {
    act(() => {
      api.mock.setCameraRoom(1, [tile(ANNA), tile(BEN)], true);
      openMedia("camera");
      openMedia("share");
      selectStage("share");
    });
  }

  it("shows the share with the cameras beside it, then above and hidden", async () => {
    const hide = vi.spyOn(api, "cameraRoomSetVideosHidden");
    render(
      <StripLayout>
        <FakeShare />
      </StripLayout>,
    );
    combined();
    expect(await screen.findByRole("radio", { name: "Cameras beside" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.getByText("2 cameras on")).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Cameras" })).not.toHaveClass("cam-strip--row");

    fireEvent.click(screen.getByRole("radio", { name: "Above" }));
    expect(screen.getByRole("list", { name: "Cameras" })).toHaveClass("cam-strip--row");
    await waitFor(async () => {
      expect((await api.settingsGet()).cameraStripPosition).toBe("above");
    });

    fireEvent.click(screen.getByRole("radio", { name: "Hidden" }));
    // Hidden unbinds the cameras; the header still counts them (FR-8.71).
    expect(screen.queryByRole("list", { name: "Cameras" })).toBeNull();
    expect(screen.getByText("2 cameras on")).toBeInTheDocument();
    await waitFor(() => {
      expect(hide).toHaveBeenLastCalledWith(1, true);
    });
  });

  it("moves through the positions with the arrow keys", async () => {
    render(
      <StripLayout>
        <FakeShare />
      </StripLayout>,
    );
    combined();
    const beside = await screen.findByRole("radio", { name: "Cameras beside" });
    fireEvent.keyDown(beside, { key: "ArrowRight" });
    expect(screen.getByRole("radio", { name: "Above" })).toHaveFocus();
    expect(screen.getByRole("radio", { name: "Above" })).toHaveAttribute("aria-checked", "true");
  });

  it("shows a strip tile large with F until Escape", async () => {
    const { container } = render(
      <StripLayout>
        <FakeShare />
      </StripLayout>,
    );
    combined();
    await screen.findByRole("list", { name: "Cameras" });
    const anna = container.querySelector<HTMLElement>(`.cam-strip [data-tile="${String(ANNA)}"]`);
    expect(anna).not.toBeNull();
    fireEvent.keyDown(anna as HTMLElement, { key: "f" });
    expect(container.querySelector(".cam-combined")).toHaveAttribute("data-large", "true");
    expect(container.querySelector(`.cam-combined__large [data-tile="${String(ANNA)}"]`)).not.toBe(
      null,
    );
    expect(screen.getByRole("button", { name: "Leave large view" })).toBeInTheDocument();

    fireEvent.keyDown(container.querySelector(".cam-combined") as HTMLElement, { key: "Escape" });
    expect(container.querySelector(".cam-combined")).not.toHaveAttribute("data-large");
    expect(container.querySelector(`.cam-strip [data-tile="${String(ANNA)}"]`)).toHaveFocus();
  });

  it("returns the cameras to the room when the share ends, without a reload", async () => {
    const hide = vi.spyOn(api, "cameraRoomSetVideosHidden");
    const { container } = render(
      <StripLayout>
        <FakeShare />
      </StripLayout>,
    );
    combined();
    await screen.findByRole("list", { name: "Cameras" });
    act(() => {
      setStripPosition("hidden");
    });
    act(() => {
      closeMedia("share");
    });
    expect(screen.queryByRole("radiogroup", { name: "Cameras" })).toBeNull();
    expect(container.querySelector(".cam-combined-off")).not.toBeNull();
    expect(mounts).toBe(1);
    // Every camera shows again in the room (FR-8.74).
    expect(hide).toHaveBeenLastCalledWith(1, false);
  });

  it("is not a strip while the camera room is the main item or popped out", () => {
    render(
      <StripLayout>
        <FakeShare />
      </StripLayout>,
    );
    act(() => {
      api.mock.setCameraRoom(1, [tile(ANNA)], true);
      openMedia("share");
      openMedia("camera");
      selectStage("camera");
    });
    expect(screen.queryByRole("radiogroup", { name: "Cameras" })).toBeNull();
  });

  it("has no axe violations", async () => {
    const { container } = render(
      <StripLayout>
        <FakeShare />
      </StripLayout>,
    );
    combined();
    await screen.findByRole("list", { name: "Cameras" });
    expect(await axeViolations(container)).toEqual([]);
  });

  it("forgets the large tile when the strip is hidden", () => {
    render(<Position />);
    act(() => {
      showLarge(BEN);
    });
    expect(screen.getByTestId("state")).toHaveTextContent(`beside ${String(BEN)}`);
    act(() => {
      setStripPosition("hidden");
    });
    expect(screen.getByTestId("state")).toHaveTextContent("hidden null");
    act(() => {
      setStripPosition("beside");
      showLarge("self");
      leaveLarge();
    });
    expect(screen.getByTestId("state")).toHaveTextContent("beside null");
  });
});
