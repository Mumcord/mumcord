import { describe, expect, it } from "vitest";
import type { Hotkey } from "../../api";
import {
  CAMERA_ACTION,
  defaultKey,
  IN_APP_SHORTCUTS,
  ownKeyFor,
  resolveDefaults,
} from "./defaultHotkeys";

const NOTHING = { macos: false, hotkeys: [], pttKey: null, cameraHotkey: null };

describe("default hotkeys (#584)", () => {
  it("gives mute, deafen, the camera, the member column and quick settings a default", () => {
    expect(defaultKey("toggleMute", false)).toBe("Ctrl+Shift+M");
    expect(defaultKey("toggleDeafen", false)).toBe("Ctrl+Shift+D");
    expect(defaultKey(CAMERA_ACTION, false)).toBe("Ctrl+Shift+C");
    expect(defaultKey("toggleMemberColumn", false)).toBe("Ctrl+Shift+U");
    expect(defaultKey("openQuickSettings", false)).toBe("Ctrl+Comma");
  });

  it("leaves push-to-talk, the voice actions and the extra rows without one", () => {
    for (const action of [
      "pushToTalk",
      "joinLeaveVoice",
      "switchVoiceServer",
      "previousChannel",
      "openMumcord",
    ] as const) {
      expect(defaultKey(action, false)).toBeNull();
    }
  });

  it("reads Ctrl as Cmd on macOS", () => {
    expect(defaultKey("toggleMute", true)).toBe("Super+Shift+M");
    expect(defaultKey("openQuickSettings", true)).toBe("Super+Comma");
  });

  it("does not take over one of Mumcord's own window shortcuts", () => {
    const keys = new Set(IN_APP_SHORTCUTS.map((entry) => entry.key));
    for (const action of ["toggleMute", "toggleDeafen", "camera", "toggleMemberColumn"] as const) {
      expect(keys.has(defaultKey(action, false) ?? "")).toBe(false);
    }
  });

  it("reports Do Not Disturb as taken by the notification centre instead of binding it", () => {
    const { active, blocked } = resolveDefaults(NOTHING);
    expect(active.has("toggleDnd")).toBe(false);
    expect(blocked.get("toggleDnd")).toEqual({
      key: "Ctrl+Shift+N",
      reason: { kind: "inApp", shortcut: "notificationCentre" },
    });
    expect(active.get("toggleMute")).toBe("Ctrl+Shift+M");
  });

  it("keeps a key the user set and drops that action's default", () => {
    const hotkeys: Hotkey[] = [{ key: "Ctrl+F7", action: { kind: "toggleMute" } }];
    const { active } = resolveDefaults({ ...NOTHING, hotkeys });
    expect(active.has("toggleMute")).toBe(false);
    expect(active.get("toggleDeafen")).toBe("Ctrl+Shift+D");
    expect(ownKeyFor("toggleMute", hotkeys, null)).toBe("Ctrl+F7");
  });

  it("switches a default off while another key uses it", () => {
    // Ctrl+Shift+D is the deafen default; the user gave it to mute instead.
    const hotkeys: Hotkey[] = [{ key: "Ctrl+Shift+D", action: { kind: "toggleMute" } }];
    const { active, blocked } = resolveDefaults({ ...NOTHING, hotkeys });
    expect(blocked.get("toggleDeafen")).toEqual({
      key: "Ctrl+Shift+D",
      reason: { kind: "other", action: { kind: "toggleMute" } },
    });
    expect(active.has("toggleDeafen")).toBe(false);
  });

  it("switches a default off while push-to-talk or the camera has its key", () => {
    const withPtt = resolveDefaults({ ...NOTHING, pttKey: "Ctrl+Shift+C" });
    expect(withPtt.blocked.get(CAMERA_ACTION)).toEqual({
      key: "Ctrl+Shift+C",
      reason: { kind: "pushToTalk" },
    });
    expect(withPtt.active.has(CAMERA_ACTION)).toBe(false);

    const withCamera = resolveDefaults({ ...NOTHING, cameraHotkey: "Ctrl+Shift+M" });
    expect(withCamera.blocked.get("toggleMute")).toEqual({
      key: "Ctrl+Shift+M",
      reason: { kind: "camera" },
    });
    // The camera keeps the key the user gave it, so its own default is not looked at.
    expect(withCamera.active.has(CAMERA_ACTION)).toBe(false);
  });
});
