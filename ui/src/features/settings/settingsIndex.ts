import { ENDONYMS } from "../../i18n/resources";
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
  /** Texts matched as they are, not translated: language names are the same in every UI language. */
  literals?: string[];
}

/** Static index of the settings screen; keep it in step with the section components. */
export const SETTINGS_INDEX: readonly SettingEntry[] = [
  {
    section: "identity",
    group: "identity.listTitle",
    terms: ["identity.intro", "identity.backupWarning"],
  },
  { section: "identity", group: "identity.listTitle", control: "identity.new" },
  {
    section: "identity",
    group: "identity.listTitle",
    control: "identity.import",
    terms: ["identity.importTitle"],
  },
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
  {
    section: "servers",
    group: "servers.connections",
    control: "servers.limit",
    terms: ["servers.limitHint"],
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
    group: "audio.quality",
    control: "audio.lowLatency",
    terms: [
      "audio.lowLatencyHint",
      "audio.lowLatencyWarningTitle",
      "audio.lowLatencyBandwidth",
      "audio.lowLatencyWifi",
      "audio.lowLatencyHeadphones",
      "audio.lowLatencyPtt",
    ],
  },
  {
    section: "audio",
    group: "audio.quality",
    control: "audio.bitrate",
    terms: ["audio.bitrateMaxKnown", "audio.bitrateMaxUnknown"],
  },
  {
    section: "audio",
    group: "audio.videoSound",
    control: "audio.ducking",
    terms: ["audio.duckingHint"],
  },
  { section: "audio", group: "audio.videoSound", control: "audio.duckingLevel" },
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
    group: "voice.whisper.title",
    terms: [
      "voice.whisper.hint",
      "voice.whisper.people",
      "voice.whisper.channel",
      "voice.whisper.hotkey",
    ],
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
    terms: [
      "voice.noiseSuppressionHint",
      "voice.noiseChoice.light",
      "voice.noiseChoice.standard",
      "voice.noiseChoice.advanced",
      "voice.noiseChoiceHint.light",
      "voice.noiseChoiceHint.standard",
      "voice.noiseChoiceHint.advanced",
      "voice.detectorHint",
    ],
    // Library names are the same in every language.
    literals: ["nnnoiseless", "rnnoise", "RNNoise", "sonora"],
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
    group: "fullImages.title",
    control: "fullImages.offer",
    terms: ["fullImages.offerHint"],
  },
  {
    section: "chatHistory",
    group: "fullImages.title",
    control: "fullImages.load",
    terms: ["fullImages.loadHint"],
  },
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
    section: "appearance",
    group: "appearanceEffects.title",
    control: "appearanceEffects.visualEffects",
    terms: ["appearanceEffects.visualEffectsHint"],
  },
  { section: "appearance", group: "channelList.title", control: "channelList.hideEmpty" },
  { section: "appearance", group: "channelList.title", control: "channelList.talkingFirst" },
  {
    section: "appearance",
    group: "channelList.title",
    control: "channelList.weakConnection",
    terms: ["channelList.weakConnectionHint"],
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
  {
    section: "language",
    control: "language.legend",
    terms: ["language.system"],
    literals: Object.values(ENDONYMS),
  },
  {
    section: "sounds",
    group: "dnd.title",
    control: "dnd.label",
    terms: ["dnd.description"],
  },
  {
    section: "sounds",
    group: "dnd.title",
    control: "dnd.follow",
    terms: ["dnd.followDescription"],
  },
  { section: "sounds", control: "sounds.joinLeave" },
  { section: "sounds", control: "sounds.voiceJoin" },
  { section: "sounds", control: "sounds.selfMute" },
  { section: "sounds", control: "sounds.selfDeaf" },
  { section: "sounds", control: "sounds.hangUp" },
  { section: "sounds", control: "sounds.messages" },
  { section: "sounds", control: "sounds.mention" },
  { section: "sounds", control: "sounds.mentionNotify" },
  { section: "sounds", control: "sounds.partyStarted" },
  { section: "sounds", control: "sounds.partyJoined" },
  { section: "sounds", control: "sounds.partyLeft" },
  { section: "sounds", control: "sounds.shareStarted" },
  { section: "sounds", control: "sounds.shareStopped" },
  { section: "sounds", control: "sounds.viewerStarted" },
  { section: "sounds", control: "sounds.viewerStopped" },
  { section: "sounds", control: "sounds.kicked" },
  { section: "sounds", control: "sounds.banned" },
  {
    section: "sounds",
    group: "notifications.title",
    control: "notifications.legend",
    terms: ["notifications.explanation"],
  },
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
  { section: "privacy", group: "blocked.title", terms: ["blocked.hint"] },
  {
    section: "privacy",
    group: "blocked.messagesTitle",
    terms: ["blocked.collapse", "blocked.hide"],
  },
  {
    section: "privacy",
    group: "privateMessages.title",
    control: "privateMessages.legend",
    terms: [
      "privateMessages.everyone",
      "privateMessages.friendsOnly",
      "privateMessages.nobody",
      "privateMessages.keepRequests",
    ],
  },
  { section: "privacy", group: "privateMessages.overrideTitle" },
  {
    section: "privacy",
    group: "privateMessages.alwaysTitle",
    terms: ["privateMessages.currentChannel", "privateMessages.accepted"],
  },
  { section: "privacy", group: "acceptedConversations.title" },
  {
    section: "privacy",
    group: "p2p.title",
    terms: [
      "p2p.modeLabel",
      "p2p.explanation",
      "p2p.mode.directRelay",
      "p2p.mode.relayFirst",
      "p2p.mode.directOnly",
      "p2p.mode.relayOnly",
    ],
  },
  {
    section: "privacy",
    group: "privacyDirect.title",
    control: "privacyDirect.allow",
    terms: ["privacyDirect.hint"],
  },
  { section: "privacy", group: "cache.title", control: "cache.clear", terms: ["cache.hint"] },
  { section: "diagnostics", group: "diagnostics.logging", control: "diagnostics.logLevel" },
  { section: "diagnostics", group: "diagnostics.file", terms: ["diagnostics.fileHint"] },
  { section: "about", group: "updates.title", control: "updates.check", terms: ["updates.hint"] },
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
        ...(entry.literals ?? []),
      ]),
    )
    .map((entry) => ({ kind: "setting", entry }));
  return [...sections, ...settings];
}
