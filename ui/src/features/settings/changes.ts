import type {
  PositionalSettings,
  PrivateMessageSettings,
  SetupPreset,
  Settings,
  SoundSettings,
} from "../../api";
import {
  applyPreset,
  presetDifferences,
  PRESET_IDS,
  type PresetDraft,
  type PresetId,
  type PresetKey,
} from "../wizard/presets";
import type { SectionId } from "./sectionIds";
import { updateModeOf } from "../updates/updateMode";

/**
 * "Changed" badges, the sidebar dots and "Reset section" (#161). Every setting the Settings
 * window shows a control for is listed once with its group and the translation key of its
 * label. A setting a preset covers is compared with the active preset, every other one with the
 * value a fresh install starts with.
 *
 * Not listed on purpose: data the user built up rather than chose (identities, custom themes,
 * blocked users, per-server rules, the push-to-talk key, skipped updates) and one-time notices.
 * A reset never deletes those.
 */
export interface TrackedSetting {
  /** Stable id, also the React key. */
  id: string;
  section: SectionId;
  /** Translation key (namespace `settings`) of the control's visible label. */
  label: string;
  /** The preset value this setting is compared with, when a preset covers it. */
  preset?: PresetKey;
  /** Whether the setting is a Beta feature. */
  beta?: boolean;
  read: (draft: PresetDraft) => unknown;
  /** `draft` with this setting taken over from `from`. */
  copy: (draft: PresetDraft, from: PresetDraft) => PresetDraft;
}

type Extra = Partial<Pick<TrackedSetting, "preset" | "beta">>;

function top(
  section: SectionId,
  label: string,
  fields: readonly (keyof Settings)[],
  extra: Extra = {},
): TrackedSetting {
  return {
    id: fields.join("+"),
    section,
    label,
    ...extra,
    read: (draft) => fields.map((field) => draft.settings[field]),
    copy: (draft, from) => ({
      ...draft,
      settings: fields.reduce<Settings>(
        (settings, field) => ({ ...settings, [field]: from.settings[field] }),
        draft.settings,
      ),
    }),
  };
}

function sound(key: keyof SoundSettings): TrackedSetting {
  return {
    id: `sounds.${key}`,
    section: "voiceAudio",
    label: `sounds.${key}`,
    read: (draft) => draft.settings.sounds[key],
    copy: (draft, from) => ({
      ...draft,
      settings: {
        ...draft.settings,
        sounds: { ...draft.settings.sounds, [key]: from.settings.sounds[key] },
      },
    }),
  };
}

function positional(key: keyof PositionalSettings, label: string): TrackedSetting {
  return {
    id: `positional.${key}`,
    section: "gaming",
    label,
    ...(key === "enabled" ? { beta: true } : {}),
    read: (draft) => draft.settings.positional[key],
    copy: (draft, from) => ({
      ...draft,
      settings: {
        ...draft.settings,
        positional: { ...draft.settings.positional, [key]: from.settings.positional[key] },
      },
    }),
  };
}

type PresetPmField = "rule" | "allowCurrentChannel" | "allowAccepted" | "keepRequests";

function privateMessages(
  key: PresetPmField & keyof PrivateMessageSettings,
  label: string,
): TrackedSetting {
  return {
    id: `privateMessages.${key}`,
    section: "chatNotifications",
    label,
    preset: `privateMessages.${key}`,
    read: (draft) => draft.settings.privateMessages[key],
    copy: (draft, from) => ({
      ...draft,
      settings: {
        ...draft.settings,
        privateMessages: {
          ...draft.settings.privateMessages,
          [key]: from.settings.privateMessages[key],
        },
      },
    }),
  };
}

const SOUND_KEYS: readonly (keyof SoundSettings)[] = [
  "joinLeave",
  "voiceJoin",
  "selfMute",
  "selfDeaf",
  "hangUp",
  "quit",
  "messages",
  "mention",
  "mentionNotify",
  "partyStarted",
  "partyJoined",
  "partyLeft",
  "shareStarted",
  "shareStopped",
  "viewerStarted",
  "viewerStopped",
  "kicked",
  "banned",
];

/** Every tracked setting, in the order the groups and cards show them. */
export const TRACKED_SETTINGS: readonly TrackedSetting[] = [
  top("general", "appearance.closeLegend", ["closeBehaviour"]),
  top("general", "appearance.autostart", ["autostart"]),
  top("general", "appearance.startMinimized", ["startMinimized"]),
  top("general", "language.legend", ["language"]),
  {
    ...top("general", "updates.mode", ["updateCheck", "updateAutoDownload"]),
    // Compared as the mode the page shows: answering the first-start question with No stores
    // "no" instead of "unset", but both mean Off, so it is no change from the preset.
    read: (draft) => updateModeOf(draft.settings),
  },
  top("general", "updates.interval", ["updateIntervalHours"]),
  top("general", "accessibility.keyboardHints", ["keyboardHints"]),
  top("general", "accessibility.announcements", ["announcements"]),
  top("general", "accessibility.skipLinks", ["skipLinks"]),
  top("account", "profile.sendToNew", ["sendDefaultAvatar"]),
  top("account", "nowPlaying.switch", ["nowPlaying"], { preset: "nowPlaying" }),
  top("account", "nowGaming.switch", ["nowGaming"], { preset: "nowGaming" }),
  top("voiceAudio", "audio.input", ["inputDevice"]),
  top("voiceAudio", "audio.output", ["outputDevice"]),
  top("voiceAudio", "voice.talkModeLegend", ["talkMode"]),
  top("voiceAudio", "voice.sensitivityLegend", ["voiceActivation", "voiceThresholdDbfs"]),
  top("voiceAudio", "voice.compatMode", ["pttCompatibilityMode"]),
  top("voiceAudio", "voice.mouseToggle", ["allowMouseButtons"]),
  top("voiceAudio", "voice.noiseSuppression", ["noiseSuppression", "noiseSuppressionBackend"]),
  top("voiceAudio", "voice.echoCancellation", ["echoCancellation"]),
  top("voiceAudio", "voice.autoGain", ["autoGain"]),
  top("voiceAudio", "audio.musicQuality", ["musicQuality"]),
  top("voiceAudio", "audio.lowLatency", ["lowLatency"], { beta: true }),
  top("voiceAudio", "audio.bitrate", ["bitrateKbps"]),
  top("voiceAudio", "voice.connectMuted", ["connectMuted"]),
  top("voiceAudio", "audio.ducking", ["videoDucking"]),
  top("voiceAudio", "audio.duckingLevel", ["videoDuckingPercent"]),
  top("voiceAudio", "sounds.theme", ["soundTheme"]),
  ...SOUND_KEYS.map(sound),
  top("chatNotifications", "notifications.legend", ["notifications"]),
  {
    id: "dndFollowSystem",
    section: "chatNotifications",
    label: "dnd.follow",
    preset: "dndFollowSystem",
    read: (draft) => draft.dndFollowSystem,
    copy: (draft, from) => ({ ...draft, dndFollowSystem: from.dndFollowSystem }),
  },
  privateMessages("rule", "privateMessages.legend"),
  privateMessages("keepRequests", "privateMessages.keepRequests"),
  privateMessages("allowCurrentChannel", "privateMessages.currentChannel"),
  privateMessages("allowAccepted", "privateMessages.accepted"),
  top("chatNotifications", "blocked.messagesTitle", ["blockedMessages"]),
  top("chatNotifications", "history.autoDelete", ["historyMaxAgeDays"]),
  top("privacy", "p2p.title", ["p2pMode"], { preset: "p2pMode" }),
  top("privacy", "privacyDirect.allow", ["allowDirectConnections"]),
  top("privacy", "linkPreviews.switch", ["linkPreviews"], { preset: "linkPreviews" }),
  top("privacy", "previewCache.after", ["previewCacheDays"]),
  top("privacy", "fullImages.offer", ["offerFullImages"], {
    preset: "offerFullImages",
    beta: true,
  }),
  top("privacy", "fullImages.load", ["loadFullImages"], { preset: "loadFullImages", beta: true }),
  top("privacy", "historySharing.switch", ["historySharing"], { preset: "historySharing" }),
  top("appearance", "appearance.modeLegend", ["theme"]),
  top("appearance", "appearance.themeLegend", ["appThemeId"]),
  top("appearance", "appearanceEffects.visualEffects", ["visualEffects"]),
  top("appearance", "appearance.messageDisplayLegend", ["chatDisplay"]),
  top("appearance", "channelList.hideEmpty", ["hideEmptyChannels"]),
  top("appearance", "channelList.talkingFirst", ["talkingFirst"]),
  top("appearance", "channelList.weakConnection", ["showWeakConnectionIcon"]),
  top("servers", "servers.limit", ["maxConnectedServers"]),
  {
    id: "ingameOverlay",
    section: "gaming",
    label: "overlay.inGame.mode",
    preset: "ingameOverlay",
    // `null`: this platform has no in-game overlay, so there is nothing to compare or reset.
    read: (draft) => draft.ingameOverlay,
    copy: (draft, from) =>
      draft.ingameOverlay === null ? draft : { ...draft, ingameOverlay: from.ingameOverlay },
  },
  positional("enabled", "positional.switch"),
  positional("minDistance", "positional.minDistance"),
  positional("maxDistance", "positional.maxDistance"),
  top("advanced", "diagnostics.logLevel", ["logLevel"]),
  top("about", "about.whatsNewAuto", ["whatsNewAuto"]),
];

/**
 * Defaults of the two preset values that live behind their own commands; they mirror the
 * engine's store defaults (overlay off, follow system Do Not Disturb on).
 */
export const DEFAULT_INGAME_OVERLAY = false;
export const DEFAULT_DND_FOLLOW_SYSTEM = true;

/** The draft a fresh install starts with, given the engine's default settings. */
export function defaultDraft(defaults: Settings, ingameOverlaySupported: boolean): PresetDraft {
  return {
    settings: defaults,
    ingameOverlay: ingameOverlaySupported ? DEFAULT_INGAME_OVERLAY : null,
    dndFollowSystem: DEFAULT_DND_FOLLOW_SYSTEM,
  };
}

/** The preset that `setupPreset` names, or `null` for "Custom" and no preset. */
export function activePresetOf(setupPreset: SetupPreset | null): PresetId | null {
  return PRESET_IDS.find((preset) => preset === setupPreset) ?? null;
}

/**
 * What every setting is compared with: the defaults, with the active preset's values on top.
 * `defaults` must carry the current overlay support (`null` when unsupported).
 */
export function baselineDraft(defaults: PresetDraft, setupPreset: SetupPreset | null): PresetDraft {
  const preset = activePresetOf(setupPreset);
  return preset === null ? defaults : applyPreset(defaults, preset);
}

function same(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** The tracked settings whose current value differs from the baseline, in display order. */
export function changedSettings(current: PresetDraft, baseline: PresetDraft): TrackedSetting[] {
  return TRACKED_SETTINGS.filter((setting) => !same(setting.read(current), setting.read(baseline)));
}

/** The groups that hold at least one changed setting (the sidebar dots). */
export function changedSections(changed: readonly TrackedSetting[]): Set<SectionId> {
  return new Set(changed.map((setting) => setting.section));
}

/**
 * "Reset section": the draft with every changed setting of `section` back at the baseline, and
 * the settings that change (for the confirm dialog).
 */
export function resetSection(
  current: PresetDraft,
  baseline: PresetDraft,
  section: SectionId,
): { draft: PresetDraft; changes: TrackedSetting[] } {
  const changes = changedSettings(current, baseline).filter(
    (setting) => setting.section === section,
  );
  const draft = changes.reduce((next, setting) => setting.copy(next, baseline), current);
  return { draft, changes };
}

/** Labels without repeats, for the lists in the confirm dialogs. */
export function uniqueLabels(settings: readonly TrackedSetting[]): string[] {
  return [...new Set(settings.map((setting) => setting.label))];
}

/** The tracked setting a preset key maps to. */
export function trackedForPresetKey(key: PresetKey): TrackedSetting | undefined {
  return TRACKED_SETTINGS.find((setting) => setting.preset === key);
}

/**
 * What switching to `preset` does: the new draft and the label keys of the settings that change.
 * Custom keeps every value; only the comparison changes (to the defaults).
 */
export function presetSwitchPlan(
  current: PresetDraft,
  preset: SetupPreset,
): { draft: PresetDraft; labels: string[] } {
  const id = activePresetOf(preset);
  if (id === null) {
    return {
      draft: { ...current, settings: { ...current.settings, setupPreset: "custom" } },
      labels: [],
    };
  }
  const labels = presetDifferences(current, id).flatMap((key) => {
    const setting = trackedForPresetKey(key);
    return setting === undefined ? [] : [setting.label];
  });
  return { draft: applyPreset(current, id), labels };
}
