import { describe, expect, it } from "vitest";
import { readableDeviceName } from "./deviceNames";
import { keyCombo, mouseKey, mouseLabel } from "./pttKey";

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
