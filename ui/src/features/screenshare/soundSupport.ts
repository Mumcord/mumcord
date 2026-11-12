/**
 * Whether this system can share sound at all (FR-7.84), asked before a share starts: the start
 * dialog and Settings show the Sound switch off and disabled, with a note naming what is missing.
 */
import { useEffect, useState } from "react";
import { getApi } from "../../api";

/** What is missing, named for the note; `null` where sound can be shared. */
export type SoundBlock = "windows" | "macos" | "pipewire" | "platform" | null;

/**
 * A failed sound capture only shows up in the live bar (FR-7.84); everything else is decided by
 * the system. Where the check itself fails, sound is treated as available and the share reports
 * the real reason.
 */
export async function loadSoundBlock(): Promise<SoundBlock> {
  const api = getApi();
  try {
    const reason = await api.screenShareSoundSupport();
    if (reason === "noPipeWire") return "pipewire";
    if (reason === "platform") return "platform";
    if (reason !== "osTooOld") return null;
    const { os } = await api.appInfo();
    return os.toLowerCase().startsWith("windows") ? "windows" : "macos";
  } catch (error: unknown) {
    console.error("Checking the share sound support failed", error);
    return null;
  }
}

/** The block, `null` until the check finished or where sound can be shared. */
export function useSoundBlock(): SoundBlock {
  const [block, setBlock] = useState<SoundBlock>(null);
  useEffect(() => {
    let active = true;
    void loadSoundBlock().then((loaded) => {
      if (active) setBlock(loaded);
    });
    return () => {
      active = false;
    };
  }, []);
  return block;
}

export const SOUND_NEEDS_KEYS = {
  windows: "soundNeeds.windows",
  macos: "soundNeeds.macos",
  pipewire: "soundNeeds.pipewire",
  platform: "soundNeeds.platform",
} as const;
