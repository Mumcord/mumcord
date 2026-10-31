import { describe, expect, it } from "vitest";
import { noteItems } from "./releaseNotes";
import {
  effectiveUpdateMode,
  packageCommand,
  updateModeOf,
  updateModePatch,
  UPDATE_MODES,
} from "./updateMode";
import { estimateSecondsLeft } from "./useUpdateStatus";

describe("update modes (FR-3.146)", () => {
  it("map to the stored settings and back", () => {
    expect(updateModePatch("off")).toEqual({ updateCheck: "no", updateAutoDownload: false });
    expect(updateModePatch("notify")).toEqual({ updateCheck: "yes", updateAutoDownload: false });
    expect(updateModePatch("auto")).toEqual({ updateCheck: "yes", updateAutoDownload: true });
    for (const mode of UPDATE_MODES) expect(updateModeOf(updateModePatch(mode))).toBe(mode);
  });

  it("count an unanswered question as Off", () => {
    expect(updateModeOf({ updateCheck: "unset", updateAutoDownload: true })).toBe("off");
  });

  it("only notify on a copy that cannot install by itself", () => {
    const auto = updateModePatch("auto");
    expect(effectiveUpdateMode(auto, false)).toBe("notify");
    expect(effectiveUpdateMode(auto, true)).toBe("auto");
  });
});

describe("package commands", () => {
  it("name the offered version", () => {
    expect(packageCommand("deb", "0.5.0")).toBe("sudo apt install ./Mumcord_0.5.0_amd64.deb");
    expect(packageCommand("rpm", "0.5.0")).toBe("sudo dnf install ./Mumcord-0.5.0-1.x86_64.rpm");
    expect(packageCommand("aur", "0.5.0")).toBe("yay -Syu mumcord-bin");
    expect(packageCommand("windowsPortable", "0.5.0")).toBeNull();
  });
});

describe("release notes", () => {
  it("turn Markdown groups into tagged plain-text entries without internal changes", () => {
    const changes = [
      "### Added",
      "- **Bold** and [a link](https://example.com) <img src=x onerror=alert(1)>",
      "### Internal",
      "- Refactoring",
      "### Fixed",
      "- Echo",
    ].join("\n");
    expect(noteItems({ version: "1.0.0", date: null, changes })).toEqual([
      { kind: "added", title: "Added", text: "Bold and a link <img src=x onerror=alert(1)>" },
      { kind: "fixed", title: "Fixed", text: "Echo" },
    ]);
  });
});

describe("time left", () => {
  const start = { atMs: 0, received: 0 };
  it("follows the average speed since the download started", () => {
    expect(estimateSecondsLeft(start, 10_000, 50, 100)).toBe(10);
  });

  it("waits for enough data and needs a known size", () => {
    expect(estimateSecondsLeft(start, 500, 50, 100)).toBeNull();
    expect(estimateSecondsLeft(start, 10_000, 50, null)).toBeNull();
    expect(estimateSecondsLeft(null, 10_000, 50, 100)).toBeNull();
  });
});
