import { SECTION_IDS, type SectionId } from "./sectionIds";

/**
 * One searchable setting. All texts are translation keys of the "settings" namespace, so the
 * search matches what the user sees in the current language.
 */
export interface SettingEntry {
  section: SectionId;
  /** Title of the group that holds the control; also used to find the control on screen. */
  group?: string;
  /** Visible label of the control (label, legend or button text). Without it, the entry is the group. */
  control?: string;
  /** Further texts that should find the entry (hints, option names); never shown in the results. */
  terms?: string[];
}

/** Static index of the settings screen; keep it in step with the section components. */
export const SETTINGS_INDEX: readonly SettingEntry[] = [
  { section: "identity", group: "identity.current", terms: ["identity.explanation"] },
  {
    section: "identity",
    group: "identity.backup",
    control: "identity.export",
    terms: ["identity.exportHint"],
  },
  { section: "identity", group: "identity.backup", control: "identity.import" },
  {
    section: "identity",
    group: "profile.title",
    terms: ["profile.defaultAvatar", "profile.choose"],
  },
  {
    section: "identity",
    group: "profile.title",
    control: "profile.sendToNew",
    terms: ["profile.sendToNewHint"],
  },
  {
    section: "identity",
    group: "nowPlaying.title",
    control: "nowPlaying.switch",
    terms: ["nowPlaying.hint"],
  },
  {
    section: "identity",
    group: "nowGaming.title",
    control: "nowGaming.switch",
    terms: ["nowGaming.hint"],
  },
  { section: "audio", group: "audio.devices", control: "audio.input" },
  { section: "audio", group: "audio.devices", control: "audio.output" },
  {
    section: "audio",
    group: "audio.quality",
    control: "audio.musicQuality",
    terms: ["audio.musicQualityHint"],
  },
  {
    section: "audio",
    group: "positional.title",
    control: "positional.switch",
    terms: ["positional.hint"],
  },
  { section: "audio", group: "positional.title", control: "positional.minDistance" },
  { section: "audio", group: "positional.title", control: "positional.maxDistance" },
  { section: "audio", group: "audio.test", terms: ["audio.testHint", "audio.testStart"] },
  {
    section: "voice",
    group: "voice.talkMode",
    control: "voice.talkModeLegend",
    terms: ["voice.voiceActivation", "voice.pushToTalk", "voice.keyCurrent"],
  },
  {
    section: "voice",
    group: "voice.talkMode",
    control: "voice.sensitivityLegend",
    terms: ["voice.automatic", "voice.manual", "voice.threshold"],
  },
  {
    section: "voice",
    group: "voice.talkMode",
    control: "voice.compatMode",
    terms: ["voice.compatModeHint"],
  },
  {
    section: "voice",
    group: "voice.talkMode",
    control: "voice.mouseToggle",
    terms: ["voice.mouseToggleWarning"],
  },
  {
    section: "voice",
    group: "voice.connecting",
    control: "voice.connectMuted",
    terms: ["voice.connectMutedHint"],
  },
  {
    section: "voice",
    group: "voice.processing",
    control: "voice.noiseSuppression",
    terms: ["voice.noiseSuppressionHint"],
  },
  {
    section: "voice",
    group: "voice.processing",
    control: "voice.echoCancellation",
    terms: ["voice.echoCancellationHint"],
  },
  {
    section: "voice",
    group: "voice.processing",
    control: "voice.autoGain",
    terms: ["voice.autoGainHint"],
  },
  {
    section: "chatHistory",
    group: "linkPreviews.title",
    control: "linkPreviews.switch",
    terms: ["linkPreviews.hint"],
  },
  {
    section: "chatHistory",
    group: "linkPreviews.title",
    control: "previewCache.after",
    terms: ["previewCache.hint"],
  },
  { section: "chatHistory", group: "history.perServer", terms: ["history.explanation"] },
  {
    section: "chatHistory",
    group: "historySharing.title",
    control: "historySharing.switch",
    terms: ["historySharing.hint"],
  },
  {
    section: "chatHistory",
    group: "history.autoDelete",
    control: "history.autoDelete",
    terms: ["history.autoDeleteHint"],
  },
  { section: "chatHistory", group: "history.all", control: "history.clearAll" },
  {
    section: "appearance",
    group: "appearance.theme",
    control: "appearance.themeLegend",
    terms: ["appearance.theme", "appearance.dark", "appearance.light"],
  },
  {
    section: "appearance",
    group: "appearance.messageDisplay",
    control: "appearance.messageDisplayLegend",
    terms: ["appearance.messageDisplay", "appearance.cozy", "appearance.compact"],
  },
  {
    section: "appearance",
    group: "appearance.behaviour",
    control: "appearance.closeLegend",
    terms: ["appearance.closeTray", "appearance.closeQuit"],
  },
  { section: "appearance", group: "appearance.behaviour", control: "appearance.autostart" },
  {
    section: "appearance",
    group: "appearance.behaviour",
    control: "appearance.startMinimized",
    terms: ["appearance.startMinimizedHint"],
  },
  {
    section: "accessibility",
    group: "accessibility.hints",
    control: "accessibility.keyboardHints",
    terms: ["accessibility.keyboardHintsHint"],
  },
  {
    section: "accessibility",
    group: "accessibility.hints",
    control: "accessibility.announcements",
    terms: ["accessibility.announcementsHint"],
  },
  {
    section: "accessibility",
    group: "accessibility.hints",
    control: "accessibility.skipLinks",
    terms: ["accessibility.skipLinksHint"],
  },
  { section: "language", control: "language.legend" },
  { section: "sounds", control: "sounds.joinLeave" },
  { section: "sounds", control: "sounds.voiceJoin" },
  { section: "sounds", control: "sounds.selfMute" },
  { section: "sounds", control: "sounds.selfDeaf" },
  { section: "sounds", control: "sounds.hangUp" },
  { section: "sounds", control: "sounds.messages" },
  { section: "sounds", control: "sounds.mention" },
  { section: "sounds", control: "sounds.mentionNotify" },
  {
    section: "sounds",
    group: "notifications.title",
    control: "notifications.legend",
    terms: ["notifications.explanation"],
  },
  {
    section: "overlay",
    group: "overlay.window.title",
    control: "overlay.window.enable",
    terms: ["overlay.window.enableHint"],
  },
  { section: "overlay", group: "overlay.window.title", control: "overlay.corner.label" },
  { section: "overlay", group: "overlay.window.title", control: "overlay.window.size" },
  { section: "overlay", group: "overlay.window.title", control: "overlay.opacity" },
  {
    section: "overlay",
    group: "overlay.window.title",
    control: "overlay.window.wholeChannel",
    terms: ["overlay.window.wholeChannelHint"],
  },
  { section: "overlay", group: "overlay.window.title", control: "overlay.window.channelName" },
  {
    section: "overlay",
    group: "overlay.inGame.title",
    terms: ["overlay.inGame.explanation", "overlay.inGame.games"],
  },
  {
    section: "audio",
    group: "gamePlugins.title",
    terms: ["gamePlugins.explanation", "gamePlugins.antiCheat"],
  },
  { section: "diagnostics", group: "diagnostics.logging", control: "diagnostics.logLevel" },
  { section: "diagnostics", group: "diagnostics.file", terms: ["diagnostics.fileHint"] },
  { section: "about", group: "about.license" },
  {
    section: "about",
    group: "about.support",
    terms: ["about.whatsNew", "about.reportBug", "about.requestFeature"],
  },
  { section: "about", group: "about.notices", terms: ["about.noticesHint"] },
];

/** A search hit: a whole section, or one setting inside it. */
export type SearchResult =
  { kind: "section"; section: SectionId } | { kind: "setting"; entry: SettingEntry };

type Translate = (key: string) => string;

/** Lower case without accents, so "Gerat" finds "Gerät" and "musica" finds "música". */
export function normalizeSearch(text: string): string {
  return text
    .normalize("NFD")
    .replace(/\p{Diacritic}/gu, "")
    .toLocaleLowerCase()
    .trim();
}

export function entryLabel(entry: SettingEntry, t: Translate): string {
  const key = entry.control ?? entry.group;
  return key === undefined ? t(`sections.${entry.section}`) : t(key);
}

/**
 * Sections whose name matches, then the settings whose label, group or hints contain every
 * word of the query.
 */
export function searchSettings(
  query: string,
  t: Translate,
  hidden: readonly SectionId[] = [],
): SearchResult[] {
  const words = normalizeSearch(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return [];
  const matches = (texts: string[]) => {
    const haystack = normalizeSearch(texts.join(" "));
    return words.every((word) => haystack.includes(word));
  };
  const sections: SearchResult[] = SECTION_IDS.filter((section) => !hidden.includes(section))
    .filter((section) => matches([t(`sections.${section}`)]))
    .map((section) => ({ kind: "section", section }));
  const settings: SearchResult[] = SETTINGS_INDEX.filter((entry) => !hidden.includes(entry.section))
    .filter((entry) =>
      matches([
        entryLabel(entry, t),
        ...(entry.group === undefined ? [] : [t(entry.group)]),
        ...(entry.terms ?? []).map(t),
      ]),
    )
    .map((entry) => ({ kind: "setting", entry }));
  return [...sections, ...settings];
}
