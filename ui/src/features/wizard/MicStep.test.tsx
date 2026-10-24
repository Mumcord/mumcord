import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { setApi, type Settings } from "../../api";
import { MicStep, PLAYBACK_MS, RECORD_MS } from "./MicStep";
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

function renderStep() {
  return renderInWizard(
    harness,
    <MicStep
      settings={settings}
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
    expect(
      await screen.findAllByRole("option", { name: "System default (PipeWire)" }),
    ).toHaveLength(2);
    expect(output).toHaveValue("");
    expect(screen.getAllByRole("option", { name: "System default" })).toHaveLength(2);
  });

  it("saves a changed device", async () => {
    renderStep();
    await screen.findAllByRole("option", { name: "USB Headset" });
    fireEvent.change(screen.getByRole("combobox", { name: "Microphone" }), {
      target: { value: "mic-usb" },
    });
    await waitFor(() => {
      expect(onDevicesChange).toHaveBeenCalledWith({ inputDevice: "mic-usb", outputDevice: null });
    });
  });

  it("reports a failed device save", async () => {
    onDevicesChange.mockRejectedValue(new Error("nope"));
    renderStep();
    await screen.findAllByRole("option", { name: "USB Headset" });
    fireEvent.change(screen.getByRole("combobox", { name: "Microphone" }), {
      target: { value: "mic-usb" },
    });
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be saved");
  });

  it("follows device hot-plug", async () => {
    renderStep();
    await screen.findAllByRole("option", { name: "USB Headset" });
    act(() => {
      harness.api.mock.emit("audioDevicesChanged", {
        devices: { inputs: [{ id: "new", name: "Webcam Mic", isDefault: false }], outputs: [] },
      });
    });
    expect(screen.getByRole("option", { name: "Webcam Mic" })).toBeInTheDocument();
  });

  it("lets the user continue when devices cannot be listed", async () => {
    harness.api.mock.failNext("audioDevices", "internal");
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    renderStep();
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be listed");
    expect(screen.getByRole("button", { name: "Next" })).toBeEnabled();
  });

  it("records, plays back and stops the test on its own", async () => {
    vi.useFakeTimers();
    const stop = vi.spyOn(harness.api, "micTestStop");
    renderStep();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Test" }));
      await Promise.resolve();
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
      vi.advanceTimersByTime(RECORD_MS);
    });
    expect(screen.getByRole("status")).toHaveTextContent("Playing back your voice…");

    await act(async () => {
      vi.advanceTimersByTime(PLAYBACK_MS);
      await Promise.resolve();
    });
    expect(stop).toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Test" })).toBeInTheDocument();
    expect(screen.getByRole("meter")).toHaveAttribute("aria-valuenow", "0");
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
    await screen.findAllByRole("option", { name: "USB Headset" });
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
