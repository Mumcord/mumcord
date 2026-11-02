import { ENDONYMS } from "../../i18n/resources";
import { SECTION_IDS, type SectionId } from "./sectionIds";

/**
 * One searchable setting. All texts are translation keys of the "settings" namespace, so the
 * search matches what the user sees in the current language.
 */
export interface SettingEntry {
  section: SectionId;
  /** Title of the card that holds the control; also used to find the control on screen. */
  group: string;
  /** Visible label of the control (label, legend or button text). Without it, the entry is the group. */
  control?: string;
  /**
   * Further texts that should find the entry (hints, option names, synonyms such as "P2P");
   * never shown in the results.
   */
  terms?: string[];
  /** Texts matched as they are, not translated: language names are the same in every UI language. */
  literals?: string[];
}

/** Static index of the settings screen; keep it in step with `SettingsPageCards`. */
export const SETTINGS_INDEX: readonly SettingEntry[] = [
  {
    section: "account",
    group: "identity.listTitle",
    terms: ["identity.intro", "identity.backupWarning"],
  },
  { section: "account", group: "identity.listTitle", control: "identity.new" },
  {
    section: "account",
    group: "identity.listTitle",
    control: "identity.import",
    terms: ["identity.importTitle"],
  },
  {
    section: "account",
    group: "profile.title",
    terms: ["profile.defaultAvatar", "profile.choose"],
  },
  {
    section: "account",
    group: "profile.title",
    control: "profile.sendToNew",
    terms: ["profile.sendToNewHint"],
  },
  {
    section: "account",
    group: "nowPlaying.title",
    control: "nowPlaying.switch",
    terms: ["nowPlaying.hint"],
  },
  {
    section: "account",
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
  {
    section: "voiceAudio",
    group: "audio.devices",
    control: "audio.input",
    terms: ["searchTerms.devices"],
  },
  {
    section: "voiceAudio",
    group: "audio.devices",
    control: "audio.output",
    terms: ["searchTerms.devices"],
  },
  {
    section: "voiceAudio",
    group: "voice.processing",
    control: "audio.musicQuality",
    terms: ["audio.musicQualityHint"],
  },
  {
    section: "voiceAudio",
    group: "voice.processing",
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
    section: "voiceAudio",
    group: "voice.processing",
    control: "audio.bitrate",
    terms: ["audio.bitrateMaxKnown", "audio.bitrateMaxUnknown"],
  },
  {
    section: "voiceAudio",
    group: "audio.videoSound",
    control: "audio.ducking",
    terms: ["audio.duckingHint"],
  },
  { section: "voiceAudio", group: "audio.videoSound", control: "audio.duckingLevel" },
  {
    section: "gaming",
    group: "positional.title",
    control: "positional.switch",
    terms: ["positional.hint"],
  },
  { section: "gaming", group: "positional.title", control: "positional.minDistance" },
  { section: "gaming", group: "positional.title", control: "positional.maxDistance" },
  { section: "voiceAudio", group: "audio.test", terms: ["audio.testHint", "audio.testStart"] },
  {
    section: "voiceAudio",
    group: "voice.talkMode",
    control: "voice.talkModeLegend",
    terms: [
      "searchTerms.talkMode",
      "voice.voiceActivation",
      "voice.pushToTalk",
      "voice.keyCurrent",
    ],
  },
  {
    section: "voiceAudio",
    group: "voice.talkMode",
    control: "voice.sensitivityLegend",
    terms: ["voice.automatic", "voice.manual", "voice.threshold"],
  },
  {
    section: "voiceAudio",
    group: "voice.talkMode",
    control: "voice.compatMode",
    terms: ["voice.compatModeHint"],
  },
  {
    section: "voiceAudio",
    group: "voice.talkMode",
    control: "voice.mouseToggle",
    terms: ["voice.mouseToggleWarning"],
  },
  {
    section: "voiceAudio",
    group: "voice.whisper.title",
    terms: [
      "voice.whisper.hint",
      "voice.whisper.people",
      "voice.whisper.channel",
      "voice.whisper.hotkey",
    ],
  },
  {
    section: "voiceAudio",
    group: "voice.connecting",
    control: "voice.connectMuted",
    terms: ["voice.connectMutedHint"],
  },
  {
    section: "voiceAudio",
    group: "voice.processing",
    control: "voice.noiseSuppression",
    terms: [
      "searchTerms.noiseSuppression",
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
    section: "voiceAudio",
    group: "voice.processing",
    control: "voice.echoCancellation",
    terms: ["voice.echoCancellationHint"],
  },
  {
    section: "voiceAudio",
    group: "voice.processing",
    control: "voice.autoGain",
    terms: ["voice.autoGainHint"],
  },
  {
    section: "voiceAudio",
    group: "sections.hotkeys",
    control: "hotkeys.background",
    terms: ["hotkeys.global", "hotkeys.backgroundHint", "hotkeys.compatLink"],
  },
  {
    section: "voiceAudio",
    group: "sections.hotkeys",
    terms: [
      "hotkeys.actions",
      "hotkeys.action.pushToTalk",
      "hotkeys.action.toggleMute",
      "hotkeys.action.toggleDeafen",
      "hotkeys.action.toggleDnd",
      "hotkeys.action.joinLeaveVoice",
      "hotkeys.action.switchVoiceServer",
      "hotkeys.action.switchChannel",
      "hotkeys.action.previousChannel",
      "hotkeys.action.toggleMemberColumn",
      "hotkeys.action.openQuickSettings",
      "hotkeys.action.openMumcord",
    ],
  },
  { section: "voiceAudio", group: "sections.hotkeys", control: "hotkeys.add" },
  { section: "voiceAudio", group: "sections.hotkeys", control: "hotkeys.reset" },
  {
    section: "privacy",
    group: "externalContent.title",
    control: "linkPreviews.switch",
    terms: ["searchTerms.externalContent", "linkPreviews.hint"],
  },
  {
    section: "privacy",
    group: "externalContent.title",
    control: "previewCache.after",
    terms: ["previewCache.hint"],
  },
  {
    section: "servers",
    group: "history.perServer",
    terms: ["history.title", "history.explanation"],
  },
  {
    section: "privacy",
    group: "externalContent.title",
    control: "fullImages.offer",
    terms: ["fullImages.offerHint"],
  },
  {
    section: "privacy",
    group: "externalContent.title",
    control: "fullImages.load",
    terms: ["fullImages.loadHint"],
  },
  {
    section: "privacy",
    group: "historySharing.title",
    control: "historySharing.switch",
    terms: ["historySharing.hint"],
  },
  {
    section: "chatNotifications",
    group: "history.title",
    control: "history.autoDelete",
    terms: ["history.autoDeleteHint"],
  },
  { section: "chatNotifications", group: "history.title", control: "history.clearAll" },
  {
    section: "appearance",
    group: "appearance.theme",
    control: "appearance.themeLegend",
    terms: ["searchTerms.theme", "appearance.theme", "appearance.dark", "appearance.light"],
  },
  {
    section: "appearance",
    group: "appearance.look",
    control: "appearance.messageDisplayLegend",
    terms: ["appearance.messageDisplay", "appearance.cozy", "appearance.compact"],
  },
  {
    section: "general",
    group: "appearance.behaviour",
    control: "appearance.closeLegend",
    terms: ["appearance.closeTray", "appearance.closeQuit"],
  },
  {
    section: "general",
    group: "appearance.behaviour",
    control: "appearance.autostart",
    terms: ["searchTerms.startup"],
  },
  {
    section: "general",
    group: "appearance.behaviour",
    control: "appearance.startMinimized",
    terms: ["appearance.startMinimizedHint"],
  },
  {
    section: "appearance",
    group: "appearance.look",
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
    section: "general",
    group: "accessibility.hints",
    control: "accessibility.keyboardHints",
    terms: ["accessibility.keyboardHintsHint"],
  },
  {
    section: "general",
    group: "accessibility.hints",
    control: "accessibility.announcements",
    terms: ["accessibility.announcementsHint"],
  },
  {
    section: "general",
    group: "accessibility.hints",
    control: "accessibility.skipLinks",
    terms: ["accessibility.skipLinksHint"],
  },
  {
    section: "general",
    group: "language.title",
    control: "language.legend",
    terms: ["searchTerms.language", "language.system"],
    literals: Object.values(ENDONYMS),
  },
  {
    section: "chatNotifications",
    group: "dnd.title",
    control: "dnd.label",
    terms: ["dnd.description"],
  },
  {
    section: "chatNotifications",
    group: "dnd.title",
    control: "dnd.follow",
    terms: ["dnd.followDescription"],
  },
  {
    section: "voiceAudio",
    group: "sounds.title",
    control: "sounds.theme",
    terms: ["sounds.themeClassic", "sounds.themeRich"],
  },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.joinLeave" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.voiceJoin" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.selfMute" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.selfDeaf" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.hangUp" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.quit" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.channelSwitch" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.messages" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.mention" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.mentionNotify" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.partyStarted" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.partyJoined" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.partyLeft" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.shareStarted" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.shareStopped" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.viewerStarted" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.viewerStopped" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.kicked" },
  { section: "voiceAudio", group: "sounds.title", control: "sounds.banned" },
  {
    section: "chatNotifications",
    group: "notifications.title",
    control: "notifications.legend",
    terms: ["searchTerms.notifications", "notifications.explanation"],
  },
  {
    section: "gaming",
    group: "overlay.inGame.title",
    terms: ["overlay.inGame.explanation", "overlay.inGame.games"],
  },
  {
    section: "gaming",
    group: "gamePlugins.title",
    terms: ["gamePlugins.explanation", "gamePlugins.antiCheat"],
  },
  {
    section: "chatNotifications",
    group: "blocked.title",
    terms: ["searchTerms.blocked", "blocked.hint"],
  },
  {
    section: "chatNotifications",
    group: "blocked.messagesTitle",
    terms: ["blocked.collapse", "blocked.hide"],
  },
  {
    section: "chatNotifications",
    group: "privateMessages.title",
    control: "privateMessages.legend",
    terms: [
      "privateMessages.everyone",
      "privateMessages.friendsOnly",
      "privateMessages.nobody",
      "privateMessages.keepRequests",
    ],
  },
  { section: "chatNotifications", group: "privateMessages.overrideTitle" },
  {
    section: "chatNotifications",
    group: "privateMessages.alwaysTitle",
    terms: ["privateMessages.currentChannel", "privateMessages.accepted"],
  },
  { section: "chatNotifications", group: "acceptedConversations.title" },
  {
    section: "privacy",
    group: "privacyConnections.title",
    control: "p2p.title",
    terms: [
      "searchTerms.p2p",
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
    group: "privacyConnections.title",
    control: "privacyDirect.allow",
    terms: ["searchTerms.p2p", "privacyDirect.hint"],
  },
  {
    section: "privacy",
    group: "privacySummary.title",
    terms: ["privacySummary.ipHidden", "privacySummary.ipVisible"],
  },
  {
    section: "advanced",
    group: "betaFeatures.title",
    terms: ["betaFeatures.hint"],
  },
  {
    section: "advanced",
    group: "betaFeatures.title",
    control: "betaFeatures.manage",
    terms: ["betaFeatures.gamePlugins"],
  },
  {
    section: "advanced",
    group: "cache.title",
    control: "cache.clear",
    terms: ["searchTerms.storage", "cache.hint"],
  },
  {
    section: "advanced",
    group: "developerMode.title",
    control: "developerMode.switch",
    terms: ["developerMode.hint"],
  },
  { section: "plugins", group: "sections.plugins", terms: ["sectionDescriptions.plugins"] },
  { section: "advanced", group: "diagnostics.title", control: "diagnostics.logLevel" },
  {
    section: "advanced",
    group: "diagnostics.title",
    terms: ["diagnostics.file", "diagnostics.fileHint"],
  },
  {
    section: "general",
    group: "updates.title",
    control: "updates.mode",
    terms: ["searchTerms.updates", "updates.modeHint", "updates.modeAuto", "updates.modeNotify"],
  },
  { section: "general", group: "updates.title", control: "updates.interval" },
  { section: "general", group: "updates.title", control: "updates.checkNow" },
  {
    section: "general",
    group: "updates.title",
    control: "updates.openChangelog",
    terms: ["updates.whatsNew"],
  },
  { section: "about", group: "about.versionTitle", terms: ["about.license"] },
  {
    section: "about",
    group: "about.support",
    terms: ["about.reportBug", "about.requestFeature"],
  },
  { section: "advanced", group: "about.notices", terms: ["about.noticesHint"] },
  {
    section: "about",
    group: "about.versionTitle",
    control: "about.whatsNewAuto",
    terms: ["about.whatsNew"],
  },
  {
    section: "about",
    group: "about.setup",
    control: "about.setupWizardRun",
    terms: ["about.setupWizardHint"],
  },
  {
    section: "advanced",
    group: "inspector.title",
    control: "inspector.devTools",
    terms: ["inspector.intro", "inspector.open", "inspector.devToolsHint"],
  },
];

/** A search hit: a whole group, or one setting inside it. */
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

const queryWords = (query: string) => normalizeSearch(query).split(/\s+/).filter(Boolean);

export function entryLabel(entry: SettingEntry, t: Translate): string {
  return t(entry.control ?? entry.group);
}

export const resultSection = (result: SearchResult): SectionId =>
  result.kind === "section" ? result.section : result.entry.section;

/**
 * Groups whose name matches, then the settings whose label, card or hints contain every word of
 * the query. Entries of cards in `hiddenCards` (title keys) are left out, as those cards are not
 * offered on this system.
 */
export function searchSettings(
  query: string,
  t: Translate,
  hiddenCards: readonly string[] = [],
): SearchResult[] {
  const words = queryWords(query);
  if (words.length === 0) return [];
  const matches = (texts: string[]) => {
    const haystack = normalizeSearch(texts.join(" "));
    return words.every((word) => haystack.includes(word));
  };
  const sections: SearchResult[] = SECTION_IDS.filter((section) =>
    matches([t(`sections.${section}`)]),
  ).map((section) => ({ kind: "section", section }));
  const settings: SearchResult[] = SETTINGS_INDEX.filter(
    (entry) => !hiddenCards.includes(entry.group),
  )
    .filter((entry) =>
      matches([
        entryLabel(entry, t),
        t(entry.group),
        ...(entry.terms ?? []).map(t),
        ...(entry.literals ?? []),
      ]),
    )
    .map((entry) => ({ kind: "setting", entry }));
  return [...sections, ...settings];
}

/** Results bucketed by group, in the order of the side list. */
export function groupResults(
  results: readonly SearchResult[],
): { section: SectionId; results: SearchResult[] }[] {
  return SECTION_IDS.map((section) => ({
    section,
    results: results.filter((result) => resultSection(result) === section),
  })).filter((group) => group.results.length > 0);
}

/**
 * Where the query's words occur in `text`, as [start, end) ranges into the original text, ignoring
 * case and accents like the search itself. Overlapping hits are merged.
 */
export function matchRanges(text: string, query: string): [number, number][] {
  const words = queryWords(query);
  // Normalized text plus, per normalized character, the index of the original character.
  let folded = "";
  const origin: number[] = [];
  let index = 0;
  for (const char of text) {
    const normal = char
      .normalize("NFD")
      .replace(/\p{Diacritic}/gu, "")
      .toLocaleLowerCase();
    for (let i = 0; i < normal.length; i++) origin.push(index);
    folded += normal;
    index += char.length;
  }
  origin.push(index);
  const ranges: [number, number][] = [];
  for (const word of words) {
    for (let at = folded.indexOf(word); at !== -1; at = folded.indexOf(word, at + 1)) {
      ranges.push([origin[at] ?? 0, origin[at + word.length] ?? text.length]);
    }
  }
  ranges.sort((a, b) => a[0] - b[0]);
  const merged: [number, number][] = [];
  for (const range of ranges) {
    const last = merged.at(-1);
    if (last && range[0] <= last[1]) last[1] = Math.max(last[1], range[1]);
    else merged.push(range);
  }
  return merged;
}
