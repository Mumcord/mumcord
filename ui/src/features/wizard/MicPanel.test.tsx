import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { setApi, type Settings } from "../../api";
import { chooseOption, optionLabels } from "../../components/SelectTestUtils";
import { MicStep } from "./MicStep";
import { renderInWizard, setupWizardTest, type Harness } from "./testUtils";

let harness: Harness;
let settings: Settings;
type DevicesChange = (devices: Pick<Settings, "inputDevice" | "outputDevice">) => Promise<void>;
let onDevicesChange: Mock<DevicesChange>;

beforeEach(async () => {
  harness = await setupWizardTest();
  settings = await harness.api.settingsGet();
  onDevicesChange = vi.fn<DevicesChange>(() => Promise.resolve());
});

afterEach(() => {
  vi.useRealTimers();
  setApi(undefined);
});

function renderStep(stagedDevices = false) {
  return renderInWizard(
    harness,
    <MicStep
      step={stagedDevices ? 1 : 2}
      total={stagedDevices ? 4 : 5}
      settings={settings}
      stagedDevices={stagedDevices}
      focusOnMount={false}
      onDevicesChange={onDevicesChange}
      onBack={vi.fn()}
      onNext={vi.fn()}
    />,
  );
}

describe("MicStep", () => {
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

  it("saves a changed device", async () => {
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

  it("tests devices that are not saved yet (#149)", async () => {
    const start = vi.spyOn(harness.api, "micTestStart");
    settings = { ...settings, inputDevice: "mic-usb" };
    renderStep(true);
    await waitFor(() => {
      expect(optionLabels(screen.getByRole("combobox", { name: "Microphone" }))).toContain(
        "USB Headset",
      );
    });
    fireEvent.click(screen.getByRole("button", { name: "Test" }));
    await waitFor(() => {
      expect(start).toHaveBeenCalledWith({ inputDevice: "mic-usb", outputDevice: null });
    });
  });

  it("reports a failed device save", async () => {
    onDevicesChange.mockRejectedValue(new Error("nope"));
    renderStep();
    const mic = screen.getByRole("combobox", { name: "Microphone" });
    await waitFor(() => {
      expect(optionLabels(mic)).toContain("USB Headset");
    });
    chooseOption(mic, "USB Headset");
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be saved");
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

  it("lets the user continue when devices cannot be listed", async () => {
    harness.api.mock.failNext("audioDevices", "internal");
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    renderStep();
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be listed");
    expect(screen.getByRole("button", { name: "Next" })).toBeEnabled();
  });

  it("follows the engine's mic test phases", async () => {
    renderStep();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Test" }));
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
    expect(screen.getByRole("button", { name: "Test" })).toBeInTheDocument();
    expect(screen.getByRole("meter")).toHaveAttribute("aria-valuenow", "0");
  });

  it("explains a test that failed after starting", async () => {
    renderStep();
    fireEvent.click(screen.getByRole("button", { name: "Test" }));
    await screen.findByRole("button", { name: "Stop" });
    act(() => {
      harness.api.mock.emit("micTestPhaseChanged", { phase: "failed" });
    });
    expect(screen.getByRole("alert")).toHaveTextContent("could not start");
    expect(screen.getByRole("button", { name: "Test" })).toBeInTheDocument();
  });

  it("stops the test with the Stop button and when leaving the step", async () => {
    const stop = vi.spyOn(harness.api, "micTestStop");
    const { unmount } = renderStep();
    fireEvent.click(screen.getByRole("button", { name: "Test" }));
    fireEvent.click(await screen.findByRole("button", { name: "Stop" }));
    await waitFor(() => {
      expect(stop).toHaveBeenCalledTimes(1);
    });
    fireEvent.click(await screen.findByRole("button", { name: "Test" }));
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
    fireEvent.click(screen.getByRole("button", { name: "Test" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("could not start");
    expect(screen.getByRole("button", { name: "Test" })).toBeInTheDocument();
  });
});
