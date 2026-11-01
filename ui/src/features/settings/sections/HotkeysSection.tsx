import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  getApi,
  toEngineError,
  type Hotkey,
  type HotkeyAction,
  type HotkeyConflict,
  type HotkeySlot,
  type HotkeysStatus,
  type PttSession,
  type WhisperSetup,
  type WhisperTarget,
} from "../../../api";
import { Select } from "../../../components/Select";
import { useServers as useConnectedServers } from "../../channels/serverStore";
import { channelPath } from "../../hotkeys/runHotkey";
import { sameAction } from "../../hotkeys/hotkeyKeys";
import { displayName } from "../../servers/format";
import { useServers } from "../../servers/store";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import { useSettingsNavigation } from "../settingsNavigation";
import { HotkeyRow, type PendingConflict } from "./HotkeyRow";
import type { SettingsSectionProps } from "./types";
import "./hotkeys.css";

type FixedKind = Exclude<HotkeyAction["kind"], "switchChannel">;

/** Actions before the "switch to channel" rows, and after them (order of mockup `epic3-64`). */
const BEFORE_CHANNELS: readonly FixedKind[] = [
  "toggleMute",
  "toggleDeafen",
  "toggleDnd",
  "joinLeaveVoice",
  "switchVoiceServer",
];
const AFTER_CHANNELS: readonly FixedKind[] = [
  "previousChannel",
  "toggleMemberColumn",
  "openQuickSettings",
  "openMumcord",
];

/** Takes the key from a whisper target (#67); the target itself stays. */
async function clearWhisperKey(targetId: string): Promise<void> {
  const { targets } = await getApi().whisperSetup();
  await getApi().whisperTargetsSave(
    targets.map((t) => (t.id === targetId ? { ...t, hotkey: null } : t)),
  );
}

/** Settings → Voice & Audio → Hotkeys card (issue #64, FR-3.72, FR-3.73): every action with its global key. */
export function HotkeysCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const [status, setStatus] = useState<HotkeysStatus | null>(null);
  const [session, setSession] = useState<PttSession | null>(null);
  const [adding, setAdding] = useState<HotkeyAction | null>(null);
  const [whisper, setWhisper] = useState<WhisperSetup | null>(null);
  const navigation = useSettingsNavigation();
  /** Conflicts waiting for "Assign here", by key. */
  const [pending] = useState(() => new Map<string, HotkeyConflict>());
  const { hotkeys, globalHotkeys } = settings;

  useEffect(() => {
    getApi()
      .pttSession()
      .then(setSession, (error: unknown) => {
        console.error("Reading the desktop session failed", error);
      });
  }, []);

  // The engine rebinds in the background after a change; ask again once it had the chance.
  useEffect(() => {
    let current = true;
    const timer = window.setTimeout(() => {
      getApi()
        .hotkeysStatus()
        .then(
          (next) => {
            if (current) setStatus(next);
          },
          (error: unknown) => {
            console.error("Reading the hotkey status failed", error);
          },
        );
    }, 300);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [hotkeys, globalHotkeys]);

  const actionName = (action: HotkeyAction) => t(`hotkeys.action.${action.kind}`);
  const conflictName = (conflict: HotkeyConflict): PendingConflict["other"] => {
    switch (conflict.kind) {
      case "pushToTalk":
        return t("hotkeys.action.pushToTalk");
      case "hotkey": {
        const other = hotkeys[conflict.index];
        return other === undefined ? null : actionName(other.action);
      }
      case "whisper":
        return conflict.name;
      case "inApp":
        return null;
    }
  };

  /** Checks a captured key; an unused one is assigned at once, a used one comes back. */
  const capture = async (
    combo: string,
    slot: HotkeySlot,
    assign: (key: string, conflict: HotkeyConflict | null) => Promise<void>,
  ): Promise<PendingConflict | null> => {
    let checked;
    try {
      checked = await getApi().hotkeyCheck(combo, slot);
    } catch (error: unknown) {
      if (toEngineError(error).code === "invalidInput") throw new Error(t("hotkeys.rejected"));
      console.error("Checking the hotkey failed", error);
      throw error;
    }
    const { key, conflict } = checked;
    if (conflict === null) {
      await assign(key, null);
      return null;
    }
    pending.set(key, conflict);
    return {
      key,
      other: conflictName(conflict),
      inApp: conflict.kind === "inApp" ? t(`hotkeys.inAppShortcut.${conflict.shortcut}`) : null,
    };
  };

  /**
   * Without the key's other owner: that hotkey goes, push-to-talk loses its key, a whisper target
   * keeps working from the user panel without one.
   */
  const freed = async (conflict: HotkeyConflict | null) => {
    if (conflict?.kind === "whisper") await clearWhisperKey(conflict.targetId);
    return {
      hotkeys:
        conflict?.kind === "hotkey" ? hotkeys.filter((_, i) => i !== conflict.index) : hotkeys,
      clearPtt: conflict?.kind === "pushToTalk",
    };
  };

  const assignAction =
    (action: HotkeyAction) => async (key: string, conflict: HotkeyConflict | null) => {
      const rest = await freed(conflict);
      const at = rest.hotkeys.findIndex((h) => sameAction(h.action, action));
      const next: Hotkey[] =
        at >= 0
          ? rest.hotkeys.map((h, i) => (i === at ? { key, action } : h))
          : [...rest.hotkeys, { key, action }];
      update(rest.clearPtt ? { hotkeys: next, pttKey: null } : { hotkeys: next });
      setAdding(null);
    };

  const assignPtt = async (key: string, conflict: HotkeyConflict | null) => {
    let pttKey;
    try {
      // On Wayland this opens the desktop's shortcut dialog.
      pttKey = await getApi().pttCaptureKey(key);
    } catch (error: unknown) {
      const code = toEngineError(error).code;
      console.error("Setting the push-to-talk key failed", error);
      throw new Error(
        code === "shortcutNotConfirmed" ? t("voice.keyNotConfirmed") : t("voice.keyRejected"),
      );
    }
    update({ pttKey, hotkeys: (await freed(conflict)).hotkeys });
  };

  const assignPending = (
    assign: (key: string, conflict: HotkeyConflict | null) => Promise<void>,
    conflict: PendingConflict,
  ) => {
    const owner = pending.get(conflict.key) ?? null;
    pending.delete(conflict.key);
    assign(conflict.key, owner).catch((error: unknown) => {
      console.error("Assigning the hotkey failed", error);
    });
  };

  // Whisper targets (#67) are set up in Voice; their keys are listed here too.
  useEffect(() => {
    getApi()
      .whisperSetup()
      .then(setWhisper, (error: unknown) => {
        console.error("Reading the whisper targets failed", error);
      });
  }, []);

  const assignWhisper =
    (targetId: string) => async (key: string | null, conflict: HotkeyConflict | null) => {
      const rest = await freed(conflict);
      const { targets } = await getApi().whisperSetup();
      setWhisper(
        await getApi().whisperTargetsSave(
          targets.map((t) => (t.id === targetId ? { ...t, hotkey: key } : t)),
        ),
      );
      if (rest.hotkeys !== hotkeys || rest.clearPtt) {
        update(rest.clearPtt ? { hotkeys: rest.hotkeys, pttKey: null } : { hotkeys: rest.hotkeys });
      }
    };

  const whisperSummary = (target: WhisperTarget) =>
    target.receivers.kind === "people"
      ? t("voice.whisper.summaryPeople", { count: target.receivers.names.length })
      : target.receivers.children
        ? t("voice.whisper.summaryChannelTree")
        : t("voice.whisper.summaryChannel");

  const whisperRow = (target: WhisperTarget) => {
    const assign = assignWhisper(target.id);
    const report = (error: unknown) => {
      console.error("Saving the whisper key failed", error);
    };
    return (
      <HotkeyRow
        key={`whisper-${target.id}`}
        label={target.name}
        description={whisperSummary(target)}
        value={target.hotkey}
        warning={
          whisper?.failedKeys.includes(target.id) === true ? t("voice.whisper.keyFailed") : null
        }
        onCapture={(combo) => capture(combo, { kind: "whisper", targetId: target.id }, assign)}
        onAssign={(conflict) => {
          assignPending(assign, conflict);
        }}
        onClear={() => {
          assign(null, null).catch(report);
        }}
      />
    );
  };

  const failedWarning = (index: number) =>
    status?.failed.includes(index) === true && globalHotkeys ? t("hotkeys.failed") : null;

  const actionRow = (action: HotkeyAction, description?: string) => {
    const index = hotkeys.findIndex((h) => sameAction(h.action, action));
    const slot: HotkeySlot = index >= 0 ? { kind: "hotkey", index } : { kind: "new" };
    const assign = assignAction(action);
    return (
      <HotkeyRow
        key={
          action.kind === "switchChannel"
            ? `${String(action.serverId)}/${action.path.join("/")}`
            : action.kind
        }
        label={actionName(action)}
        description={description}
        value={hotkeys[index]?.key ?? null}
        warning={index >= 0 ? failedWarning(index) : null}
        onCapture={(combo) => capture(combo, slot, assign)}
        onAssign={(conflict) => {
          assignPending(assign, conflict);
        }}
        onClear={() => {
          update({ hotkeys: hotkeys.filter((_, i) => i !== index) });
        }}
      />
    );
  };

  const savedServers = useServers().servers;
  const serverName = (serverId: number) => {
    const server = savedServers.find((s) => s.id === serverId);
    return server === undefined ? null : displayName(server);
  };
  const channelDescription = (action: Extract<HotkeyAction, { kind: "switchChannel" }>) => {
    const channel = action.path.length === 0 ? t("hotkeys.rootChannel") : action.path.join(" / ");
    const server = serverName(action.serverId);
    return server === null ? channel : t("hotkeys.channelPlace", { channel, server });
  };
  const channelRows = hotkeys
    .map((h) => h.action)
    .filter(
      (a): a is Extract<HotkeyAction, { kind: "switchChannel" }> => a.kind === "switchChannel",
    );

  const portalNote =
    status?.mode === "portal"
      ? "hotkeys.portal"
      : status?.mode === "unavailable"
        ? "hotkeys.noPortal"
        : null;
  const compatAvailable = session?.waylandDesktop === true && !session.compatibilityActive;

  return (
    <Group title={t("sections.hotkeys")} icon="key" wide>
      <h4 className="hotkeys-subheading">{t("hotkeys.global")}</h4>
      <Switch
        label={t("hotkeys.background")}
        description={t("hotkeys.backgroundHint")}
        checked={globalHotkeys}
        onChange={(next) => {
          update({ globalHotkeys: next });
        }}
      />
      {!globalHotkeys && <p className="settings-hint">{t("hotkeys.foregroundOnly")}</p>}
      {globalHotkeys && portalNote !== null && (
        <p className="hotkeys-note" role="status">
          {t(portalNote)}
          {compatAvailable && navigation !== null && (
            <>
              {" "}
              <button
                type="button"
                className="settings-link"
                onClick={navigation.openCompatibilityMode}
              >
                {t("hotkeys.compatLink")}
              </button>{" "}
              {t("hotkeys.compatSuffix")}
            </>
          )}
        </p>
      )}
      <h4 className="hotkeys-subheading">{t("hotkeys.actions")}</h4>
      <HotkeyRow
        label={t("hotkeys.action.pushToTalk")}
        description={
          settings.talkMode === "pushToTalk"
            ? t("hotkeys.pushToTalkHint")
            : t("hotkeys.pushToTalkOff")
        }
        value={settings.pttKey}
        onCapture={(combo) => capture(combo, { kind: "pushToTalk" }, assignPtt)}
        onAssign={(conflict) => {
          assignPending(assignPtt, conflict);
        }}
        onClear={() => {
          update({ pttKey: null });
        }}
      />
      {BEFORE_CHANNELS.map((kind) => actionRow({ kind }))}
      {channelRows.map((action) => actionRow(action, channelDescription(action)))}
      {adding !== null && adding.kind === "switchChannel" && (
        <HotkeyRow
          label={actionName(adding)}
          description={channelDescription(adding)}
          value={null}
          startRecording
          onCapture={(combo) => capture(combo, { kind: "new" }, assignAction(adding))}
          onAssign={(conflict) => {
            assignPending(assignAction(adding), conflict);
          }}
          onClear={() => {
            setAdding(null);
          }}
          onCancel={() => {
            setAdding(null);
          }}
        />
      )}
      {AFTER_CHANNELS.map((kind) => actionRow({ kind }))}
      {whisper?.targets.map(whisperRow)}
      <AddChannelHotkey
        taken={channelRows}
        onPick={(action) => {
          setAdding(action);
        }}
      />
      <div className="settings-actions">
        <button
          type="button"
          className="settings-button"
          disabled={hotkeys.length === 0}
          onClick={() => {
            update({ hotkeys: [] });
          }}
        >
          {t("hotkeys.reset")}
        </button>
      </div>
      <p className="settings-hint">{t("hotkeys.resetHint")}</p>
    </Group>
  );
}

/** "Add 'switch to channel' shortcut…": pick a channel of a connected server, then its key. */
function AddChannelHotkey({
  taken,
  onPick,
}: {
  taken: readonly Extract<HotkeyAction, { kind: "switchChannel" }>[];
  onPick: (action: HotkeyAction) => void;
}) {
  const { t } = useTranslation("settings");
  const id = useId();
  const [open, setOpen] = useState(false);
  const [choice, setChoice] = useState("");
  const connected = useConnectedServers();
  const savedServers = useServers().servers;

  const options: { value: string; label: string; action: HotkeyAction }[] = [];
  for (const [serverId, state] of connected) {
    if (!state.connected) continue;
    const saved = savedServers.find((s) => s.id === serverId);
    const server = saved === undefined ? String(serverId) : displayName(saved);
    for (const channel of state.channels.values()) {
      const path = channelPath(state, channel.id);
      const action: HotkeyAction = { kind: "switchChannel", serverId, path };
      if (taken.some((a) => sameAction(a, action))) continue;
      const name = path.length === 0 ? t("hotkeys.rootChannel") : path.join(" / ");
      options.push({
        value: `${String(serverId)}:${String(channel.id)}`,
        label: t("hotkeys.channelPlace", { channel: name, server }),
        action,
      });
    }
  }
  options.sort((a, b) => a.label.localeCompare(b.label));

  if (!open) {
    return (
      <button
        type="button"
        className="settings-link hotkeys-add"
        onClick={() => {
          setOpen(true);
        }}
      >
        {t("hotkeys.add")}
      </button>
    );
  }
  const selected = options.find((o) => o.value === choice) ?? options[0];
  if (selected === undefined) {
    return <p className="settings-hint">{t("hotkeys.addNone")}</p>;
  }
  return (
    <div className="hotkeys-add-picker">
      <label htmlFor={`${id}-channel`}>{t("hotkeys.addChannel")}</label>
      <Select
        id={`${id}-channel`}
        value={selected.value}
        options={options.map(({ value, label }) => ({ value, label }))}
        onChange={setChoice}
      />
      <div className="settings-actions">
        <button
          type="button"
          className="settings-button settings-button--primary"
          onClick={() => {
            setOpen(false);
            setChoice("");
            onPick(selected.action);
          }}
        >
          {t("hotkeys.addConfirm")}
        </button>
        <button
          type="button"
          className="settings-button"
          onClick={() => {
            setOpen(false);
          }}
        >
          {t("hotkeys.cancel")}
        </button>
      </div>
    </div>
  );
}
