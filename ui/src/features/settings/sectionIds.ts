/** Settings sections in the order of the side list. */
export const SECTION_IDS = [
  "identity",
  "audio",
  "voice",
  "chatHistory",
  "appearance",
  "accessibility",
  "language",
  "sounds",
  "overlay",
  "diagnostics",
  "about",
] as const;
export type SectionId = (typeof SECTION_IDS)[number];
