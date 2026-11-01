/** Settings sections in the order of the side list. */
export const SECTION_IDS = [
  "identity",
  "servers",
  "audio",
  "voice",
  "hotkeys",
  "chatHistory",
  "appearance",
  "accessibility",
  "language",
  "sounds",
  "overlay",
  "privacy",
  "updates",
  "diagnostics",
  "about",
  "packetInspector",
] as const;
export type SectionId = (typeof SECTION_IDS)[number];
