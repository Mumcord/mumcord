/**
 * The camera preview dialog (FR-8.10–8.14): it starts the local preview, lets the user pick a
 * camera and a background, turns the camera on and keeps it off on Cancel / Escape.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../../api";
import { axeViolations } from "../../../test/axe";
import { connectedMock, resetAll, stubDialog } from "../../channels/testUtils";
import { CameraPreviewHost, openCameraPreview, useCameraPreviewTarget } from ".";
import { resetCameraPreview } from "../previewStore";

let api: MockApi;

beforeAll(() => {
  stubDialog();
  // jsdom has no Tauri IPC: the picture stays empty, which the dialog reports with a warning.
  vi.spyOn(console, "warn").mockImplementation(() => undefined);
});

afterEach(() => {
  act(() => {
    resetCameraPreview();
  });
  resetAll();
});

async function openDialog() {
  api = await connectedMock({ bindStore: true });
  const view = render(<CameraPreviewHost />);
  act(() => {
    openCameraPreview(1, null);
  });
  const dialog = await screen.findByRole("dialog", { name: "Turn on your camera" });
  const turnOn = within(dialog).getByRole("button", { name: /Turn on camera/ });
  await waitFor(() => {
    expect(turnOn).toBeEnabled();
  });
  return { dialog, turnOn, view };
}

describe("CameraPreviewDialog", () => {
  it("starts the preview without sending and names who will see it", async () => {
    const { dialog } = await openDialog();
    expect(within(dialog).getByText("Preview · nobody sees this yet")).toBeInTheDocument();
    expect(dialog).toHaveTextContent(/Only people in .+ see your camera/);
    expect(within(dialog).getByRole("combobox", { name: "Camera" })).toHaveValue("mock-integrated");
    expect((await api.cameraView())?.state).toBe("preview");
    expect(within(dialog).getByRole("radio", { name: "Blur" })).toBeChecked();
    const strength = within(dialog).getByRole("group", { name: "Blur strength" });
    expect(within(strength).getByRole("button", { name: "Light" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("switches the background and shows the image library", async () => {
    const { dialog } = await openDialog();
    const setBackground = vi.spyOn(api, "cameraSetBackground");
    fireEvent.click(within(dialog).getByRole("radio", { name: "Image" }));
    expect(setBackground).toHaveBeenLastCalledWith(
      { kind: "builtin", id: "abstract-warm-waves" },
      50,
    );
    const library = within(dialog).getByRole("radiogroup", { name: "Image" });
    await within(library).findByRole("radio", { name: "Your image 1" });
    expect(within(library).getAllByRole("radio")).toHaveLength(19);
    fireEvent.click(within(library).getByRole("radio", { name: "Modern WC" }));
    expect(setBackground).toHaveBeenLastCalledWith({ kind: "builtin", id: "modern-wc" }, 50);
    expect(within(dialog).getByRole("button", { name: "Add your own image" })).toBeEnabled();
    fireEvent.click(within(dialog).getByRole("radio", { name: "None" }));
    expect(setBackground).toHaveBeenLastCalledWith({ kind: "none" }, 50);
    expect(within(dialog).queryByRole("radiogroup", { name: "Image" })).toBeNull();
  });

  it("maps Light and Strong to fixed strengths and saves the choice", async () => {
    const { dialog, turnOn } = await openDialog();
    const setBackground = vi.spyOn(api, "cameraSetBackground");
    const strength = within(dialog).getByRole("group", { name: "Blur strength" });
    fireEvent.click(within(strength).getByRole("button", { name: "Strong" }));
    expect(setBackground).toHaveBeenLastCalledWith({ kind: "blur" }, 100);
    expect(within(strength).getByRole("button", { name: "Strong" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    fireEvent.click(turnOn);
    await waitFor(
      () => {
        expect(screen.queryByRole("dialog")).toBeNull();
      },
      { timeout: 3000 },
    );
    expect((await api.settingsGet()).cameraBlurStrength).toBe(100);
  });

  it("turns the camera on, saves the background and closes once on air", async () => {
    const { dialog, turnOn } = await openDialog();
    fireEvent.click(within(dialog).getByRole("radio", { name: "None" }));
    fireEvent.click(turnOn);
    await waitFor(() => {
      expect(within(dialog).getAllByText("Starting camera …").length).toBeGreaterThan(0);
    });
    expect(turnOn).toBeDisabled();
    await waitFor(
      () => {
        expect(screen.queryByRole("dialog")).toBeNull();
      },
      { timeout: 3000 },
    );
    expect((await api.cameraView())?.state).toBe("onAir");
    expect((await api.settingsGet()).cameraDefaultBackground).toEqual({ kind: "none" });
  });

  it("keeps the dialog open with the reason when turning on fails", async () => {
    const { dialog, turnOn } = await openDialog();
    api.mock.failNext("cameraTurnOn", "camera", "roomFull");
    fireEvent.click(turnOn);
    expect(
      await within(dialog).findByText("The camera room is full (9 of 9 cameras)."),
    ).toBeVisible();
    await waitFor(() => {
      expect(turnOn).toBeEnabled();
    });
  });

  it("closes the dialog before turning the camera off on Escape", async () => {
    const { dialog } = await openDialog();
    let target: ReturnType<typeof useCameraPreviewTarget> | undefined;
    function Probe() {
      target = useCameraPreviewTarget();
      return null;
    }
    render(<Probe />);
    const openWhenTurnedOff: boolean[] = [];
    const turnOff = api.cameraTurnOff.bind(api);
    const off = vi.spyOn(api, "cameraTurnOff").mockImplementation((serverId) => {
      openWhenTurnedOff.push(target !== null);
      return turnOff(serverId);
    });
    act(() => {
      dialog.dispatchEvent(new Event("cancel", { cancelable: true }));
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(off).toHaveBeenCalledWith(1);
    expect(openWhenTurnedOff).toEqual([false]);
  });

  it("closes the dialog before turning the camera off on Cancel", async () => {
    const { dialog } = await openDialog();
    const off = vi.spyOn(api, "cameraTurnOff");
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(off).toHaveBeenCalledWith(1);
  });

  it("does not save the background when 'Use this background next time' is off", async () => {
    const { dialog, turnOn } = await openDialog();
    fireEvent.click(within(dialog).getByRole("radio", { name: "None" }));
    fireEvent.click(
      within(dialog).getByRole("checkbox", { name: "Use this background next time" }),
    );
    fireEvent.click(turnOn);
    await waitFor(
      () => {
        expect(screen.queryByRole("dialog")).toBeNull();
      },
      { timeout: 3000 },
    );
    expect((await api.settingsGet()).cameraDefaultBackground).toEqual({ kind: "blur" });
  });

  it("has no accessibility violations", async () => {
    const { dialog } = await openDialog();
    fireEvent.click(within(dialog).getByRole("radio", { name: "Image" }));
    await within(dialog).findByRole("radio", { name: "Your image 1" });
    expect(await axeViolations(dialog)).toEqual([]);
  });
});
