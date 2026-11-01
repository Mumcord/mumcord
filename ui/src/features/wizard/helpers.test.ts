import { describe, expect, it } from "vitest";
import { shortDeviceName } from "./deviceName";

const via = (backend: string) => `System default (${backend})`;
describe("shortDeviceName", () => {
  it("shortens the ALSA default device", () => {
    expect(shortDeviceName("Default ALSA Output (currently PipeWire Media Server)", via)).toBe(
      "System default (PipeWire)",
    );
    expect(shortDeviceName("Default ALSA Input (currently PulseAudio Sound Server)", via)).toBe(
      "System default (PulseAudio)",
    );
  });

  it("keeps the first line of other names", () => {
    expect(shortDeviceName("HDA Intel PCH, ALC892 Analog\nDefault Audio Device", via)).toBe(
      "HDA Intel PCH, ALC892 Analog",
    );
    expect(shortDeviceName("USB Headset", via)).toBe("USB Headset");
  });
});
