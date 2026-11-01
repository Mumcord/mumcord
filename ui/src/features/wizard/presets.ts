import type { P2pMode, PmRule, Settings, SetupPreset } from "../../api";

/**
 * Setup style presets of the wizard (issue #159). A preset sets a fixed list of values; every
 * other setting keeps what it has. Two of the values are not part of `Settings` but live behind
 * their own commands: the in-game overlay (`ingameOverlaySetMode`) and "Follow system Do Not
 * Disturb" (`dndSetFollowSystem`), so the wizard works on a `PresetDraft` that carries both.
 *
 * Not covered on purpose: "external images: click to load" and "embeds" from the issue table have
 * no setting of their own (`linkPreviews` covers previews and embeds), and Maximum privacy's
 * "Updates: notify only" waits for that setting (#295), so updates are left untouched.
 */

/** The three presets; Custom walks the detailed steps instead of applying values. */
export type PresetId = Exclude<SetupPreset, "custom">;

/** Card order on the setup style step. */
export const PRESET_IDS: readonly PresetId[] = ["everythingOn", "balanced", "maximumPrivacy"];

/** The four setup styles in card order: the three presets, then Custom. */
export const SETUP_STYLES: readonly SetupPreset[] = [...PRESET_IDS, "custom"];

/** The preset the wizard selects at first: Balanced, marked "Recommended". */
export const RECOMMENDED_PRESET: PresetId = "balanced";

/** Every value a preset sets, keyed like the settings it maps to. */
export interface PresetValues {
  "privateMessages.rule": PmRule;
  "privateMessages.allowCurrentChannel": boolean;
  "privateMessages.allowAccepted": boolean;
  "privateMessages.keepRequests": boolean;
  p2pMode: P2pMode;
  linkPreviews: boolean;
  offerFullImages: boolean;
  loadFullImages: boolean;
  historySharing: boolean;
  nowPlaying: boolean;
  nowGaming: boolean;
  /** In-game overlay mode "on". */
  ingameOverlay: boolean;
  /** "Follow system Do Not Disturb". */
  dndFollowSystem: boolean;
}

export type PresetKey = keyof PresetValues;

/** The keys in display order, so difference lists read the same everywhere. */
export const PRESET_KEYS: readonly PresetKey[] = [
  "privateMessages.rule",
  "privateMessages.allowCurrentChannel",
  "privateMessages.allowAccepted",
  "privateMessages.keepRequests",
  "p2pMode",
  "linkPreviews",
  "offerFullImages",
  "loadFullImages",
  "historySharing",
  "nowPlaying",
  "nowGaming",
  "ingameOverlay",
  "dndFollowSystem",
];

export const PRESETS: Readonly<Record<PresetId, Readonly<PresetValues>>> = {
  everythingOn: {
    "privateMessages.rule": "everyone",
    "privateMessages.allowCurrentChannel": true,
    "privateMessages.allowAccepted": true,
    "privateMessages.keepRequests": true,
    p2pMode: "directRelay",
    linkPreviews: true,
    offerFullImages: true,
    loadFullImages: true,
    historySharing: true,
    nowPlaying: true,
    nowGaming: true,
    ingameOverlay: true,
    dndFollowSystem: true,
  },
  balanced: {
    "privateMessages.rule": "friendsOnly",
    "privateMessages.allowCurrentChannel": true,
    "privateMessages.allowAccepted": true,
    "privateMessages.keepRequests": true,
    p2pMode: "directRelay",
    linkPreviews: true,
    offerFullImages: true,
    loadFullImages: true,
    historySharing: true,
    nowPlaying: false,
    nowGaming: false,
    ingameOverlay: true,
    dndFollowSystem: true,
  },
  maximumPrivacy: {
    "privateMessages.rule": "friendsOnly",
    "privateMessages.allowCurrentChannel": false,
    "privateMessages.allowAccepted": false,
    "privateMessages.keepRequests": false,
    p2pMode: "relayOnly",
    linkPreviews: false,
    offerFullImages: false,
    loadFullImages: false,
    historySharing: false,
    nowPlaying: false,
    nowGaming: false,
    ingameOverlay: false,
    dndFollowSystem: true,
  },
};

/** What the wizard edits: the settings record plus the two values behind their own commands. */
export interface PresetDraft {
  settings: Settings;
  /**
   * In-game overlay on, or `null` where this platform cannot draw inside games: then the
   * overlay is neither set by a preset nor counted as a difference.
   */
  ingameOverlay: boolean | null;
  dndFollowSystem: boolean;
}

/** The current value of one preset key in a draft (`null`: in-game overlay unsupported). */
export function presetValueOf<K extends PresetKey>(
  draft: PresetDraft,
  key: K,
): PresetValues[K] | null {
  const { settings } = draft;
  const values: { [P in PresetKey]: PresetValues[P] | null } = {
    "privateMessages.rule": settings.privateMessages.rule,
    "privateMessages.allowCurrentChannel": settings.privateMessages.allowCurrentChannel,
    "privateMessages.allowAccepted": settings.privateMessages.allowAccepted,
    "privateMessages.keepRequests": settings.privateMessages.keepRequests,
    p2pMode: settings.p2pMode,
    linkPreviews: settings.linkPreviews,
    offerFullImages: settings.offerFullImages,
    loadFullImages: settings.loadFullImages,
    historySharing: settings.historySharing,
    nowPlaying: settings.nowPlaying,
    nowGaming: settings.nowGaming,
    ingameOverlay: draft.ingameOverlay,
    dndFollowSystem: draft.dndFollowSystem,
  };
  return values[key];
}

/**
 * The draft with every value of `preset` set and `setupPreset` recorded. Everything else,
 * including per-server PM rules, stays as it is.
 */
export function applyPreset(draft: PresetDraft, preset: PresetId): PresetDraft {
  const values = PRESETS[preset];
  const { settings } = draft;
  return {
    settings: {
      ...settings,
      privateMessages: {
        ...settings.privateMessages,
        rule: values["privateMessages.rule"],
        allowCurrentChannel: values["privateMessages.allowCurrentChannel"],
        allowAccepted: values["privateMessages.allowAccepted"],
        keepRequests: values["privateMessages.keepRequests"],
      },
      p2pMode: values.p2pMode,
      linkPreviews: values.linkPreviews,
      offerFullImages: values.offerFullImages,
      loadFullImages: values.loadFullImages,
      historySharing: values.historySharing,
      nowPlaying: values.nowPlaying,
      nowGaming: values.nowGaming,
      setupPreset: preset,
    },
    ingameOverlay: draft.ingameOverlay === null ? null : values.ingameOverlay,
    dndFollowSystem: values.dndFollowSystem,
  };
}

/**
 * The keys whose current value differs from `preset`, in `PRESET_KEYS` order; its length is
 * the N of "Balanced, customised: N changed".
 */
export function presetDifferences(draft: PresetDraft, preset: PresetId): PresetKey[] {
  const values = PRESETS[preset];
  return PRESET_KEYS.filter((key) => {
    const current = presetValueOf(draft, key);
    return current !== null && current !== values[key];
  });
}

/** The preset the draft's values match exactly, or `null` when none does. */
export function matchingPreset(draft: PresetDraft): PresetId | null {
  return PRESET_IDS.find((preset) => presetDifferences(draft, preset).length === 0) ?? null;
}
