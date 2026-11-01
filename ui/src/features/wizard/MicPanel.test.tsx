import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { setApi, type Settings } from "../../api";
import { chooseOption, optionLabels } from "../../components/SelectTestUtils";
import { MicPanel } from "./MicPanel";
import { renderInWizard, setupWizardTest, type Harness } from "./testUtils";

let harness: Harness;
let settings: Settings;
type DevicesChange = (devices: Pick<Settings, "inputDevice" | "outputDevice">) => void;
let onDevicesChange: Mock<DevicesChange>;

beforeEach(async () => {
  harness = await setupWizardTest();
  settings = await harness.api.settingsGet();
  onDevicesChange = vi.fn<DevicesChange>();
});

afterEach(() => {
  vi.useRealTimers();
  setApi(undefined);
});

function renderStep() {
  return renderInWizard(
    harness,
    <MicPanel settings={settings} onDevicesChange={onDevicesChange} />,
  );
}

describe("MicPanel", () => {
  it("lists devices with short names and a system default", async () => {
    vi.spyOn(harness.api, "audioDevices").mockResolvedValue({
      inputs: [
        {
          id: "default",
          name: "Default ALSA Input (currently PipeWire Media Server)",
          isDefault: true,
        },
      ],
      outputs: [
        {
          id: "default",
          name: "Default ALSA Output (currently PipeWire Media Server)",
          isDefault: true,
        },
      ],
    });
    renderStep();
    const output = screen.getByRole("combobox", { name: "Speakers or headphones" });
    const input = await screen.findByRole("combobox", { name: "Microphone" });
    await waitFor(() => {
      expect(optionLabels(input)).toContain("System default (PipeWire)");
    });
    expect(output).toHaveTextContent("System default");
    expect(optionLabels(output)).toContain("System default");
  });

  it("reports a changed device", async () => {
    renderStep();
    const mic = screen.getByRole("combobox", { name: "Microphone" });
    await waitFor(() => {
      expect(optionLabels(mic)).toContain("USB Headset");
    });
    chooseOption(mic, "USB Headset");
    await waitFor(() => {
      expect(onDevicesChange).toHaveBeenCalledWith({ inputDevice: "mic-usb", outputDevice: null });
    });
  });

  it("tests the shown devices without saving them", async () => {
    const start = vi.spyOn(harness.api, "micTestStart");
    settings = { ...settings, inputDevice: "mic-usb" };
    renderStep();
    await waitFor(() => {
      expect(optionLabels(screen.getByRole("combobox", { name: "Microphone" }))).toContain(
        "USB Headset",
      );
    });
    fireEvent.click(screen.getByRole("button", { name: "Hear yourself" }));
    await waitFor(() => {
      expect(start).toHaveBeenCalledWith({ inputDevice: "mic-usb", outputDevice: null });
    });
  });

  it("follows device hot-plug", async () => {
    renderStep();
    const mic = screen.getByRole("combobox", { name: "Microphone" });
    await waitFor(() => {
      expect(optionLabels(mic)).toContain("USB Headset");
    });
    act(() => {
      harness.api.mock.emit("audioDevicesChanged", {
        devices: { inputs: [{ id: "new", name: "Webcam Mic", isDefault: false }], outputs: [] },
      });
    });
    expect(optionLabels(mic)).toContain("Webcam Mic");
  });

  it("reports devices that cannot be listed", async () => {
    harness.api.mock.failNext("audioDevices", "internal");
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    renderStep();
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be listed");
    expect(screen.getByRole("button", { name: "Hear yourself" })).toBeEnabled();
  });

  it("follows the engine's mic test phases", async () => {
    renderStep();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Hear yourself" }));
      await Promise.resolve();
    });
    act(() => {
      harness.api.mock.emit("micTestPhaseChanged", { phase: "recording" });
    });
    expect(screen.getByRole("status")).toHaveTextContent("Speak now…");

    act(() => {
      harness.api.mock.emit("micLevel", { level: 0.42 });
    });
    expect(screen.getByRole("meter", { name: "Microphone level" })).toHaveAttribute(
      "aria-valuenow",
      "42",
    );

    act(() => {
      harness.api.mock.emit("micTestPhaseChanged", { phase: "playing" });
    });
    expect(screen.getByRole("status")).toHaveTextContent("Playing back your voice…");

    act(() => {
      harness.api.mock.emit("micTestPhaseChanged", { phase: "done" });
    });
    expect(screen.getByRole("button", { name: "Hear yourself" })).toBeInTheDocument();
    expect(screen.getByRole("meter")).toHaveAttribute("aria-valuenow", "0");
  });

  it("explains a test that failed after starting", async () => {
    renderStep();
    fireEvent.click(screen.getByRole("button", { name: "Hear yourself" }));
    await screen.findByRole("button", { name: "Stop" });
    act(() => {
      harness.api.mock.emit("micTestPhaseChanged", { phase: "failed" });
    });
    expect(screen.getByRole("alert")).toHaveTextContent("could not start");
    expect(screen.getByRole("button", { name: "Hear yourself" })).toBeInTheDocument();
  });

  it("stops the test with the Stop button and when leaving the step", async () => {
    const stop = vi.spyOn(harness.api, "micTestStop");
    const { unmount } = renderStep();
    fireEvent.click(screen.getByRole("button", { name: "Hear yourself" }));
    fireEvent.click(await screen.findByRole("button", { name: "Stop" }));
    await waitFor(() => {
      expect(stop).toHaveBeenCalledTimes(1);
    });
    fireEvent.click(await screen.findByRole("button", { name: "Hear yourself" }));
    await screen.findByRole("button", { name: "Stop" });
    unmount();
    expect(stop).toHaveBeenCalledTimes(2);
  });

  it("ignores level events while no test runs", async () => {
    renderStep();
    const mic = screen.getByRole("combobox", { name: "Microphone" });
    await waitFor(() => {
      expect(optionLabels(mic)).toContain("USB Headset");
    });
    act(() => {
      harness.api.mock.emit("micLevel", { level: 0.9 });
    });
    expect(screen.getByRole("meter")).toHaveAttribute("aria-valuenow", "0");
  });

  it("explains a failed test start", async () => {
    harness.api.mock.failNext("micTestStart", "internal");
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    renderStep();
    fireEvent.click(screen.getByRole("button", { name: "Hear yourself" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("could not start");
    expect(screen.getByRole("button", { name: "Hear yourself" })).toBeInTheDocument();
  });
});
