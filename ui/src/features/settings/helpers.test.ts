import { describe, expect, it } from "vitest";
import { readableDeviceName } from "./deviceNames";
import { keyCombo } from "./pttKey";

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

describe("keyCombo", () => {
  it("joins modifiers and the key code", () => {
    expect(keyCombo(key("KeyA", { ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+KeyA");
    expect(keyCombo(key("F13"))).toBe("F13");
    expect(keyCombo(key("Space", { metaKey: true, altKey: true }))).toBe("Alt+Super+Space");
  });

  it("ignores modifier-only presses", () => {
    expect(keyCombo(key("ShiftLeft", { shiftKey: true }))).toBeNull();
    expect(keyCombo(key("ControlRight", { ctrlKey: true }))).toBeNull();
    expect(keyCombo(key(""))).toBeNull();
  });
});

describe("readableDeviceName", () => {
  it("drops the ALSA '(currently …)' suffix", () => {
    expect(readableDeviceName("Default ALSA Output (currently PipeWire Media Server)")).toBe(
      "Default ALSA Output",
    );
    expect(readableDeviceName("USB Headset")).toBe("USB Headset");
  });
});
