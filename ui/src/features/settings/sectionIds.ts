import type { SettingsIconName } from "./components/SettingsIcon";

/** Settings groups in the order of the side list (#161). */
export const SECTION_IDS = [
  "general",
  "account",
  "voiceAudio",
  "chatNotifications",
  "privacy",
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
  chatNotifications: "message",
  privacy: "shield",
  appearance: "palette",
  servers: "server",
  gaming: "gamepad",
  advanced: "code",
  about: "info",
};
