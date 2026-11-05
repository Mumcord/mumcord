import { beforeAll, describe, expect, it } from "vitest";
import { EngineError } from "../../../api";
import { applyLanguage, i18n } from "../../../i18n";
import {
  backgroundForMode,
  backgroundMode,
  cameraFailureOf,
  canRetryAccess,
  desktopOs,
  failureText,
  initialDevice,
  isStrongBlur,
  loadLine,
  loadLineText,
  sameBackground,
} from "./previewLogic";

const t = () => i18n.getFixedT("en", "camera");

beforeAll(async () => {
  await applyLanguage("en");
});

describe("cameraFailureOf", () => {
  it("reads notConnected and the camera reasons", () => {
    expect(cameraFailureOf(new EngineError("notConnected", "x"))).toBe("notConnected");
    expect(cameraFailureOf(new EngineError("camera", "roomFull"))).toBe("roomFull");
  });

  it("falls back to cameraFailed for anything else", () => {
    expect(cameraFailureOf(new EngineError("camera", "somethingNew"))).toBe("cameraFailed");
    expect(cameraFailureOf(new EngineError("internal", "roomFull"))).toBe("cameraFailed");
    expect(cameraFailureOf(new Error("boom"))).toBe("cameraFailed");
  });
});

describe("failureText", () => {
  it("uses the FR-8.51 texts for the answers to Turn on camera", () => {
    expect(failureText(t(), "roomFull", "linux")).toBe("The camera room is full (9 of 9 cameras).");
    expect(failureText(t(), "relayFull", "linux")).toBe(
      "Camera couldn't start: this server's relay is full right now.",
    );
    expect(failureText(t(), "useNotAllowed", "linux")).toBe(
      "This server's relay doesn't allow camera sharing.",
    );
    expect(failureText(t(), "denied", "linux")).toBe(
      "Camera couldn't start: the server's relay refused it.",
    );
    expect(failureText(t(), "relayDown", "linux")).toBe(
      "Camera couldn't start: the relay isn't reachable.",
    );
    expect(failureText(t(), "rateLimited", "linux")).toBe(
      "Camera couldn't start: too many attempts. Try again in a minute.",
    );
  });

  it("names the camera access block of each OS", () => {
    expect(failureText(t(), "permissionDenied", "windows")).toBe(
      "Windows blocks camera access for desktop apps",
    );
    expect(failureText(t(), "permissionDenied", "macos")).toBe(
      "macOS blocks camera access for Mumcord",
    );
    expect(failureText(t(), "permissionDenied", "linux")).toBe("Camera access was denied");
    expect(canRetryAccess("permissionDenied", "linux")).toBe(true);
    expect(canRetryAccess("permissionDenied", "windows")).toBe(false);
    expect(canRetryAccess("deviceGone", "linux")).toBe(false);
  });

  it("asks to join a voice channel when not connected", () => {
    expect(failureText(t(), "notConnected", null)).toBe(
      "Join a voice channel to turn on your camera.",
    );
  });
});

describe("desktopOs", () => {
  it("reads the OS family", () => {
    expect(desktopOs("windows x86_64")).toBe("windows");
    expect(desktopOs("macos aarch64")).toBe("macos");
    expect(desktopOs("linux x86_64")).toBe("linux");
    expect(desktopOs("")).toBeNull();
  });
});

describe("backgrounds", () => {
  it("maps backgrounds to the three modes", () => {
    expect(backgroundMode({ kind: "none" })).toBe("none");
    expect(backgroundMode({ kind: "blur" })).toBe("blur");
    expect(backgroundMode({ kind: "builtin", id: "beach" })).toBe("image");
    expect(backgroundMode({ kind: "own", id: "a" })).toBe("image");
  });

  it("returns to the last image when Image is picked again", () => {
    expect(backgroundForMode("image", null)).toEqual({
      kind: "builtin",
      id: "abstract-warm-waves",
    });
    expect(backgroundForMode("image", { kind: "own", id: "a" })).toEqual({ kind: "own", id: "a" });
    expect(backgroundForMode("blur", { kind: "own", id: "a" })).toEqual({ kind: "blur" });
  });

  it("compares backgrounds by kind and id", () => {
    expect(sameBackground({ kind: "blur" }, { kind: "blur" })).toBe(true);
    expect(sameBackground({ kind: "own", id: "a" }, { kind: "own", id: "b" })).toBe(false);
    expect(sameBackground({ kind: "own", id: "a" }, { kind: "builtin", id: "a" })).toBe(false);
  });
});

describe("initialDevice", () => {
  const devices = [
    { id: "a", name: "A", isDefault: false },
    { id: "b", name: "B", isDefault: true },
  ];

  it("prefers the remembered camera, then the system default", () => {
    expect(initialDevice(devices, "a")?.id).toBe("a");
    expect(initialDevice(devices, "gone")?.id).toBe("b");
    expect(initialDevice(devices, null)?.id).toBe("b");
    expect(initialDevice([], null)).toBeUndefined();
  });
});

describe("loadLine", () => {
  const effects = { unavailable: null, level: null, load: null, tooBusy: false } as const;

  it("shows the load and the level that runs (FR-8.36)", () => {
    const line = loadLine({ ...effects, level: "best", load: "low" });
    expect(line).toEqual({ kind: "load", load: "low", level: "best" });
    expect(loadLineText(t(), line ?? { kind: "idle" })).toBe(
      "Effects run on this device. Load: low (Best)",
    );
    const light = loadLine({ ...effects, level: "light", load: "high" });
    expect(loadLineText(t(), light ?? { kind: "idle" })).toBe(
      "Effects run on this device. Load: high (Light)",
    );
  });

  it("says why effects are off, and when the PC is too busy", () => {
    const off = loadLine({ ...effects, unavailable: "ONNX Runtime is missing" });
    expect(loadLineText(t(), off ?? { kind: "idle" })).toBe(
      "Background effects are not available on this PC: ONNX Runtime is missing",
    );
    const busy = loadLine({ ...effects, load: "high", level: "light", tooBusy: true });
    expect(loadLineText(t(), busy ?? { kind: "idle" })).toBe(
      "Your PC is too busy for this background effect. Try None.",
    );
    expect(loadLine(null)).toBeNull();
  });
});

describe("isStrongBlur", () => {
  it("treats anything above Light as Strong", () => {
    expect(isStrongBlur(50)).toBe(false);
    expect(isStrongBlur(0)).toBe(false);
    expect(isStrongBlur(100)).toBe(true);
    expect(isStrongBlur(75)).toBe(true);
  });
});
