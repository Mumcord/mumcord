import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type HistoryScope, type SavedServer } from "../../../api";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Group, Section } from "../components/Section";
import { Switch } from "../components/Switch";
import { errorText } from "../errors";

type Status = { kind: "success" | "error"; text: string } | null;

/** Chat & history: "Save history" and "Clear history" per server, plus clearing everything. */
export function ChatHistorySection() {
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
