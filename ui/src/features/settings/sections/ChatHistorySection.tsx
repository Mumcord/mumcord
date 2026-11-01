import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type HistoryScope, type SavedServer } from "../../../api";
import { BetaBadge } from "../../../components/BetaBadge";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { DaysSelect } from "../components/DaysSelect";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import { errorText } from "../errors";
import { formatSize } from "../formatSize";
import type { SettingsSectionProps } from "./types";

type Status = { kind: "success" | "error"; text: string } | null;

const CACHE_DAYS = [1, 7, 30] as const;
const HISTORY_DAYS = [7, 30, 90, 365] as const;

function StatusLine({ status }: { status: Status }) {
  return (
    <p role="status" className={status?.kind === "error" ? "settings-error" : "settings-success"}>
      {status?.text}
    </p>
  );
}

/** "Clear history" for one server or all of them, always after a confirmation. */
function useClearHistory(setStatus: (status: Status) => void) {
  const { t } = useTranslation("settings");
  const [clearing, setClearing] = useState<{ scope: HistoryScope; label: string | null } | null>(
    null,
  );

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

  const request = (scope: HistoryScope, label: string | null) => {
    setStatus(null);
    setClearing({ scope, label });
  };

  const dialog =
    clearing === null ? null : (
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
    );
  return { request, dialog };
}

/**
 * Link previews (FR-1.49) with their cache clean-up, and full-size images (FR-1.74): everything
 * that loads content from outside the server.
 */
export function ExternalContentCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const [status, setStatus] = useState<Status>(null);

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

  return (
    <Group title={t("externalContent.title")} icon="link">
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
        description={t("previewCache.hint")}
        value={settings.previewCacheDays}
        options={CACHE_DAYS}
        noneLabel={t("previewCache.never")}
        onChange={(previewCacheDays) => {
          update({ previewCacheDays });
        }}
      />
      <div className="settings-actions">
        <button type="button" className="settings-button" onClick={() => void clearCache()}>
          {t("previewCache.clearNow")}
        </button>
      </div>
      <StatusLine status={status} />
      <Switch
        label={t("fullImages.offer")}
        badge={<BetaBadge />}
        description={t("fullImages.offerHint")}
        checked={settings.offerFullImages}
        onChange={(offerFullImages) => {
          update({ offerFullImages });
        }}
      />
      <Switch
        label={t("fullImages.load")}
        badge={<BetaBadge />}
        description={t("fullImages.loadHint")}
        checked={settings.loadFullImages}
        onChange={(loadFullImages) => {
          update({ loadFullImages });
        }}
      />
    </Group>
  );
}

/** "Share my channel history" with other Mumcord users (FR-1.60). */
export function HistorySyncCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Group title={t("historySharing.title")} icon="refresh">
      <Switch
        label={t("historySharing.switch")}
        description={t("historySharing.hint")}
        checked={settings.historySharing}
        onChange={(historySharing) => {
          update({ historySharing });
        }}
      />
    </Group>
  );
}

/** "Save history" and "Clear history" per saved server. */
export function PerServerHistoryCard() {
  const { t } = useTranslation("settings");
  const [servers, setServers] = useState<SavedServer[] | null>(null);
  const [status, setStatus] = useState<Status>(null);
  const clearing = useClearHistory(setStatus);

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

  return (
    <Group title={t("history.perServer")} icon="server" info={t("history.explanation")} wide>
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
                clearing.request({ kind: "server", serverId: server.id }, server.label);
              }}
            >
              {t("history.clearServer", { server: server.label })}
            </button>
          </li>
        ))}
      </ul>
      <StatusLine status={status} />
      {clearing.dialog}
    </Group>
  );
}

/** Deleting old history automatically (QA.42) and clearing all of it. */
export function MessageHistoryCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const [status, setStatus] = useState<Status>(null);
  const clearing = useClearHistory(setStatus);
  return (
    <Group title={t("history.title")} icon="clock" info={t("history.explanation")}>
      <DaysSelect
        label={t("history.autoDelete")}
        description={t("history.autoDeleteHint")}
        value={settings.historyMaxAgeDays}
        options={HISTORY_DAYS}
        noneLabel={t("history.autoDeleteOff")}
        onChange={(historyMaxAgeDays) => {
          update({ historyMaxAgeDays });
        }}
      />
      <div className="settings-actions">
        <button
          type="button"
          className="settings-button settings-button--danger"
          onClick={() => {
            clearing.request({ kind: "all" }, null);
          }}
        >
          {t("history.clearAll")}
        </button>
      </div>
      <StatusLine status={status} />
      {clearing.dialog}
    </Group>
  );
}
