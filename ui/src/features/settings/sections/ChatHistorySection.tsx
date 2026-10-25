import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type HistoryScope, type SavedServer } from "../../../api";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { DaysSelect } from "../components/DaysSelect";
import { Group, Section } from "../components/Section";
import { Switch } from "../components/Switch";
import { errorText } from "../errors";
import type { SettingsSectionProps } from "./types";

type Status = { kind: "success" | "error"; text: string } | null;

const CACHE_DAYS = [1, 7, 30] as const;
const HISTORY_DAYS = [7, 30, 90, 365] as const;

/** "1.4 MB" style size for the space a cache clean-up freed. */
function formatSize(bytes: number): string {
  const units = ["B", "KB", "MB", "GB"];
  let size = bytes;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${unit === 0 ? String(size) : size.toFixed(1)} ${units[unit] ?? "B"}`;
}

/**
 * Chat & history: link previews (FR-1.49) with their cache clean-up, "Share my channel history"
 * with other Mumcord users (FR-1.60), "Save history" and "Clear history" per server, plus
 * clearing everything and deleting old history automatically (QA.42).
 */
export function ChatHistorySection({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const [servers, setServers] = useState<SavedServer[] | null>(null);
  const [clearing, setClearing] = useState<{ scope: HistoryScope; label: string | null } | null>(
    null,
  );
  const [status, setStatus] = useState<Status>(null);

  useEffect(() => {
    let active = true;
    getApi()
      .serversList()
      .then(
        (loaded) => {
          if (active) setServers(loaded);
        },
        (error: unknown) => {
          console.error("Loading servers failed", error);
          if (active) setStatus({ kind: "error", text: errorText(t, error) });
        },
      );
    return () => {
      active = false;
    };
  }, [t]);

  const setSaving = async (server: SavedServer, enabled: boolean) => {
    const apply = (value: boolean) => {
      setServers(
        (list) => list?.map((s) => (s.id === server.id ? { ...s, saveHistory: value } : s)) ?? null,
      );
    };
    apply(enabled);
    try {
      await getApi().historySetSaving(server.id, enabled);
    } catch (error: unknown) {
      console.error("Changing history saving failed", error);
      apply(!enabled);
      setStatus({ kind: "error", text: errorText(t, error) });
    }
  };

  const clearCache = async () => {
    setStatus(null);
    try {
      const freed = await getApi().previewCacheClear();
      setStatus({ kind: "success", text: t("previewCache.cleared", { size: formatSize(freed) }) });
    } catch (error: unknown) {
      console.error("Clearing the preview cache failed", error);
      setStatus({ kind: "error", text: errorText(t, error) });
    }
  };

  const clear = async (scope: HistoryScope) => {
    setClearing(null);
    try {
      await getApi().historyClear(scope);
      setStatus({ kind: "success", text: t("history.cleared") });
    } catch (error: unknown) {
      console.error("Clearing history failed", error);
      setStatus({ kind: "error", text: errorText(t, error) });
    }
  };

  return (
    <Section title={t("sections.chatHistory")}>
      <Group title={t("linkPreviews.title")}>
        <Switch
          label={t("linkPreviews.switch")}
          description={t("linkPreviews.hint")}
          checked={settings.linkPreviews}
          onChange={(linkPreviews) => {
            update({ linkPreviews });
          }}
        />
        <DaysSelect
          label={t("previewCache.after")}
          value={settings.previewCacheDays}
          options={CACHE_DAYS}
          noneLabel={t("previewCache.never")}
          onChange={(previewCacheDays) => {
            update({ previewCacheDays });
          }}
        />
        <p className="settings-hint">{t("previewCache.hint")}</p>
        <button type="button" className="settings-button" onClick={() => void clearCache()}>
          {t("previewCache.clearNow")}
        </button>
      </Group>
      <Group title={t("historySharing.title")}>
        <Switch
          label={t("historySharing.switch")}
          description={t("historySharing.hint")}
          checked={settings.historySharing}
          onChange={(historySharing) => {
            update({ historySharing });
          }}
        />
      </Group>
      <p className="settings-hint">{t("history.explanation")}</p>
      <Group title={t("history.perServer")}>
        {servers?.length === 0 && <p className="settings-hint">{t("history.noServers")}</p>}
        <ul className="settings-list">
          {servers?.map((server) => (
            <li key={server.id} className="settings-list__item">
              <Switch
                label={t("history.save", { server: server.label })}
                checked={server.saveHistory}
                onChange={(enabled) => void setSaving(server, enabled)}
              />
              <button
                type="button"
                className="settings-button"
                onClick={() => {
                  setStatus(null);
                  setClearing({
                    scope: { kind: "server", serverId: server.id },
                    label: server.label,
                  });
                }}
              >
                {t("history.clearServer", { server: server.label })}
              </button>
            </li>
          ))}
        </ul>
      </Group>
      <Group title={t("history.autoDelete")}>
        <DaysSelect
          label={t("history.autoDelete")}
          value={settings.historyMaxAgeDays}
          options={HISTORY_DAYS}
          noneLabel={t("history.autoDeleteOff")}
          onChange={(historyMaxAgeDays) => {
            update({ historyMaxAgeDays });
          }}
        />
        <p className="settings-hint">{t("history.autoDeleteHint")}</p>
      </Group>
      <Group title={t("history.all")}>
        <button
          type="button"
          className="settings-button settings-button--danger"
          onClick={() => {
            setStatus(null);
            setClearing({ scope: { kind: "all" }, label: null });
          }}
        >
          {t("history.clearAll")}
        </button>
      </Group>
      <p role="status" className={status?.kind === "error" ? "settings-error" : "settings-success"}>
        {status?.text}
      </p>
      {clearing !== null && (
        <ConfirmDialog
          title={t("history.confirmTitle")}
          message={
            clearing.label === null
              ? t("history.confirmAll")
              : t("history.confirmServer", { server: clearing.label })
          }
          confirmLabel={t("history.confirm")}
          onConfirm={() => void clear(clearing.scope)}
          onCancel={() => {
            setClearing(null);
          }}
        />
      )}
    </Section>
  );
}
