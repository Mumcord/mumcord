import { describe, expect, it } from "vitest";
import { shortDeviceName } from "./deviceName";
import { keyCombo, mouseKey, mouseLabel } from "./keyCombo";

const via = (backend: string) => `System default (${backend})`;
const key = (
  code: string,
  mods: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>> = {},
) => ({
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

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

describe("keyCombo", () => {
  it("joins modifiers in a fixed order before the key code", () => {
    expect(keyCombo(key("KeyA"))).toBe("KeyA");
    expect(keyCombo(key("KeyV", { shiftKey: true, ctrlKey: true }))).toBe("Ctrl+Shift+KeyV");
    expect(keyCombo(key("Space", { metaKey: true, altKey: true }))).toBe("Alt+Meta+Space");
  });

  it("waits while only a modifier is pressed", () => {
    expect(keyCombo(key("ControlLeft", { ctrlKey: true }))).toBeNull();
    expect(keyCombo(key("ShiftRight", { shiftKey: true }))).toBeNull();
    expect(keyCombo(key(""))).toBeNull();
  });
});

describe("mouseKey", () => {
  it("maps middle and side buttons to pttKey names", () => {
    expect(mouseKey(1)).toBe("MouseMiddle");
    expect(mouseKey(3)).toBe("Mouse4");
    expect(mouseKey(4)).toBe("Mouse5");
  });

  it("leaves left and right click alone", () => {
    expect(mouseKey(0)).toBeNull();
    expect(mouseKey(2)).toBeNull();
    expect(mouseKey(7)).toBeNull();
  });

  it("labels stored mouse keys only", () => {
    expect(mouseLabel("Mouse4")).toBe("back");
    expect(mouseLabel("Mouse5")).toBe("forward");
    expect(mouseLabel("MouseMiddle")).toBe("middle");
    expect(mouseLabel("Ctrl+A")).toBeNull();
  });
});
