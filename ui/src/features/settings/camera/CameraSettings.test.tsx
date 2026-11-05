import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi, type Settings } from "../../../api";
import { chooseOption, optionLabels } from "../../../components/SelectTestUtils";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { BackgroundCard, CameraCard, CameraPrivacyCard, OthersCamerasCard } from ".";
import { reduceMotionOn, shownDeviceId } from "./cameraSettings";

let api: MockApi;
let settings: Settings;

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi({ seed: "demo" });
  setApi(api);
  settings = await api.settingsGet();
});

afterEach(() => {
  setApi(undefined);
  vi.restoreAllMocks();
});

function Page({ update }: { update: (patch: Partial<Settings>) => void }) {
  return (
    <>
      <CameraCard settings={settings} update={update} />
      <BackgroundCard settings={settings} update={update} />
      <CameraPrivacyCard settings={settings} update={update} />
      <OthersCamerasCard settings={settings} update={update} />
    </>
  );
}

describe("Settings → Camera (FR-8.75)", () => {
  it("shows the mockup's controls with their defaults", async () => {
    render(<Page update={vi.fn()} />);
    const camera = screen.getByRole("combobox", { name: "Camera" });
    await waitFor(() => {
      expect(optionLabels(camera)).toEqual(["Webcam", "USB C920"]);
    });
    expect(optionLabels(screen.getByRole("combobox", { name: "Send quality" }))).toEqual([
      "Auto",
      "720p · 15 fps",
      "360p · 30 fps",
    ]);
    expect(screen.getByRole("switch", { name: "Mirror my self-view" })).toBeChecked();
    expect(
      screen.getByRole("switch", { name: /Turn my camera off when my screen locks/ }),
    ).toBeChecked();
    expect(optionLabels(screen.getByRole("combobox", { name: "Show cameras" }))).toEqual([
      "Always",
      "When I open the room",
      "Never",
    ]);
    expect(optionLabels(screen.getByRole("combobox", { name: "Effect quality" }))).toEqual([
      "Auto",
      "Best (graphics card)",
      "Light (saves battery)",
    ]);
    expect(screen.getByText(/Mumcord never records cameras/)).toBeInTheDocument();
    await screen.findByText(/\(2 saved\)/);
  });

  it("saves a new device and switches the running preview to it", async () => {
    const setDevice = vi.spyOn(api, "cameraSetDevice");
    const update = vi.fn();
    render(<Page update={update} />);
    const camera = screen.getByRole("combobox", { name: "Camera" });
    await waitFor(() => {
      expect(optionLabels(camera)).toContain("USB C920");
    });
    chooseOption(camera, "USB C920");
    expect(update).toHaveBeenCalledWith({ cameraDeviceId: "mock-usb" });
    expect(setDevice).toHaveBeenCalledWith("mock-usb");
  });

  it("saves the default background and applies it to the preview", async () => {
    const setBackground = vi.spyOn(api, "cameraSetBackground");
    const update = vi.fn();
    render(<Page update={update} />);
    const select = screen.getByRole("combobox", { name: "Default background" });
    await waitFor(() => {
      expect(optionLabels(select)).toEqual([
        "None",
        "Blur",
        "Image: Library",
        "Image: Image 1 (yours)",
        "Image: Image 2 (yours)",
      ]);
    });
    chooseOption(select, "Image: Library");
    expect(update).toHaveBeenCalledWith({
      cameraDefaultBackground: { kind: "builtin", id: "beach" },
    });
    expect(setBackground).toHaveBeenCalledWith({ kind: "builtin", id: "beach" }, 50);
  });

  it("saves the blur strength when the slider is let go", () => {
    const update = vi.fn();
    render(<Page update={update} />);
    const slider = screen.getByRole("slider", { name: "Blur strength" });
    fireEvent.change(slider, { target: { value: "80" } });
    expect(update).not.toHaveBeenCalled();
    fireEvent.blur(slider);
    expect(update).toHaveBeenCalledWith({ cameraBlurStrength: 80 });
  });

  it("stores reduce motion as an explicit choice", () => {
    const update = vi.fn();
    render(<Page update={update} />);
    fireEvent.click(screen.getByRole("switch", { name: /Reduce motion/ }));
    expect(update).toHaveBeenCalledWith({ cameraReduceMotion: "on" });
  });

  it("deletes the default background image and falls back to Blur (FR-8.15)", async () => {
    settings = { ...settings, cameraDefaultBackground: { kind: "own", id: "own-1" } };
    const update = vi.fn();
    render(<Page update={update} />);
    fireEvent.click(await screen.findByRole("button", { name: "Manage images" }));
    const dialog = await screen.findByRole("dialog", { name: "Your background images" });
    expect(dialog).toBeInTheDocument();
    fireEvent.click(await screen.findByRole("button", { name: "Delete Image 1" }));
    await waitFor(() => {
      expect(update).toHaveBeenCalledWith({ cameraDefaultBackground: { kind: "blur" } });
    });
    expect(await api.cameraOwnBackgrounds()).toHaveLength(1);
  });

  it("adds an image and explains a file that can't be used", async () => {
    const prompt = vi.spyOn(window, "prompt");
    render(<Page update={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Manage images" }));
    prompt.mockReturnValueOnce("/home/me/garden.jpg");
    fireEvent.click(await screen.findByRole("button", { name: "Add image" }));
    expect(await screen.findByRole("button", { name: "Delete Image 3" })).toBeInTheDocument();
    prompt.mockReturnValueOnce("/home/me/notes.txt");
    fireEvent.click(screen.getByRole("button", { name: "Add image" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("This image can't be used");
  });

  it("stops the local preview when the page closes (FR-8.30)", async () => {
    const start = vi.spyOn(api, "cameraLocalPreviewStart");
    const stop = vi.spyOn(api, "cameraLocalPreviewStop");
    const { unmount } = render(<Page update={vi.fn()} />);
    await waitFor(() => {
      expect(start).toHaveBeenCalledTimes(1);
    });
    expect(stop).not.toHaveBeenCalled();
    act(() => {
      unmount();
    });
    expect(stop).toHaveBeenCalledTimes(1);
  });

  it("explains why there is no preview", async () => {
    setApi(createMockApi());
    render(<Page update={vi.fn()} />);
    expect(await screen.findByRole("img", { name: /No camera found/ })).toBeInTheDocument();
  });

  it("has no axe violations", async () => {
    const { container } = render(<Page update={vi.fn()} />);
    await screen.findByText(/\(2 saved\)/);
    expect(await axeViolations(container)).toEqual([]);
  });
});

describe("camera settings helpers", () => {
  it("shows the saved camera, else the system default", () => {
    const devices = [
      { id: "a", name: "A", isDefault: false },
      { id: "b", name: "B", isDefault: true },
    ];
    expect(shownDeviceId(devices, "a")).toBe("a");
    expect(shownDeviceId(devices, "gone")).toBe("b");
    expect(shownDeviceId(devices, null)).toBe("b");
    expect(shownDeviceId([], null)).toBe("");
  });

  it("follows the system's reduce motion until the user chooses", () => {
    expect(reduceMotionOn("system", true)).toBe(true);
    expect(reduceMotionOn("system", false)).toBe(false);
    expect(reduceMotionOn("on", false)).toBe(true);
    expect(reduceMotionOn("off", true)).toBe(false);
  });
});
