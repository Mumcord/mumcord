import type { SettingsIconName } from "./components/SettingsIcon";

/** Settings groups in the order of the side list (#161). */
export const SECTION_IDS = [
  "general",
  "account",
  "voiceAudio",
  "screenSharing",
  "camera",
  "chatNotifications",
  "privacy",
  "plugins",
  "appearance",
  "servers",
  "gaming",
  "advanced",
  "about",
] as const;
export type SectionId = (typeof SECTION_IDS)[number];

/** Groups set apart below a divider at the end of the side list. */
export const SET_APART_SECTIONS: readonly SectionId[] = ["advanced", "about"];

export const SECTION_ICONS: Record<SectionId, SettingsIconName> = {
  general: "settings",
  account: "user",
  voiceAudio: "mic",
  screenSharing: "monitor",
  camera: "camera",
  chatNotifications: "message",
  privacy: "shield",
  plugins: "puzzle",
  appearance: "palette",
  servers: "server",
  gaming: "gamepad",
  advanced: "code",
  about: "info",
};

/** Groups that carry a "New" pill in the side list until the next release cycle. */
export const NEW_SECTIONS: readonly SectionId[] = ["screenSharing", "camera"];
