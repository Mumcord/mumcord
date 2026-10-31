import type { InstallKind, Settings } from "../../api";

/** The three choices of "When an update is found" (FR-3.146). */
export type UpdateMode = "auto" | "notify" | "off";

export const UPDATE_MODES: readonly UpdateMode[] = ["auto", "notify", "off"];

type ModeSettings = Pick<Settings, "updateCheck" | "updateAutoDownload">;

/** The mode the stored settings stand for; a question not answered yet counts as Off. */
export function updateModeOf(settings: ModeSettings): UpdateMode {
  if (settings.updateCheck !== "yes") return "off";
  return settings.updateAutoDownload ? "auto" : "notify";
}

/** The settings fields that store `mode`. */
export function updateModePatch(mode: UpdateMode): ModeSettings {
  switch (mode) {
    case "auto":
      return { updateCheck: "yes", updateAutoDownload: true };
    case "notify":
      return { updateCheck: "yes", updateAutoDownload: false };
    case "off":
      return { updateCheck: "no", updateAutoDownload: false };
  }
}

/**
 * The mode as shown: a copy that cannot install by itself only ever notifies, even when
 * "Download automatically" was stored on another kind of install.
 */
export function effectiveUpdateMode(settings: ModeSettings, canInstall: boolean): UpdateMode {
  const mode = updateModeOf(settings);
  return mode === "auto" && !canInstall ? "notify" : mode;
}

/** Check intervals offered in Settings, in hours (FR-3.144). */
export const UPDATE_INTERVALS = [6, 12, 24, 168] as const;

/** The command that installs `version` on packages updated by their package manager. */
export function packageCommand(kind: InstallKind, version: string): string | null {
  switch (kind) {
    case "deb":
      return `sudo apt install ./Mumcord_${version}_amd64.deb`;
    case "rpm":
      return `sudo dnf install ./Mumcord-${version}-1.x86_64.rpm`;
    case "aur":
      return "yay -Syu mumcord-bin";
    default:
      return null;
  }
}
