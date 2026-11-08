import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type LogLevel } from "../../../api";
import { Select } from "../../../components/Select";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import { errorText } from "../errors";
import { pickSavePath } from "../fileDialogs";
import type { SettingsSectionProps } from "./types";
import "../crash/crash.css";

const LEVELS: LogLevel[] = ["error", "warn", "info", "debug"];

type Status = { kind: "success" | "error"; text: string } | null;

/** Log level and "Save diagnostic file" (FR-1.40). */
export function DiagnosticsCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  const [status, setStatus] = useState<Status>(null);

  const save = async () => {
    setStatus(null);
    try {
      const path = await pickSavePath(
        "mumcord-diagnostics.txt",
        { name: t("diagnostics.fileType"), extensions: ["txt"] },
        t("diagnostics.save"),
      );
      if (path === null) return;
      await getApi().diagnosticsSave(path);
      setStatus({ kind: "success", text: t("diagnostics.saved") });
    } catch (error: unknown) {
      console.error("Saving the diagnostic file failed", error);
      setStatus({ kind: "error", text: errorText(t, error) });
    }
  };

  const copyLog = async (whole: boolean) => {
    setStatus(null);
    try {
      await getApi().copyText(await getApi().logText(whole));
      setStatus({ kind: "success", text: t("diagnostics.logCopied") });
    } catch (error: unknown) {
      console.error("Copying the log failed", error);
      setStatus({ kind: "error", text: errorText(t, error) });
    }
  };

  const openLogFolder = async () => {
    try {
      await getApi().logOpenFolder();
    } catch (error: unknown) {
      console.error("Opening the log folder failed", error);
      setStatus({ kind: "error", text: errorText(t, error) });
    }
  };

  return (
    <Group title={t("diagnostics.title")} icon="file" info={t("diagnostics.fileHint")}>
      <Switch
        label={t("diagnostics.writeToDisk")}
        description={t("diagnostics.writeToDiskHint")}
        checked={settings.logToDisk}
        onChange={(logToDisk) => {
          update({ logToDisk });
        }}
      />
      <div className="settings-control-row">
        <label htmlFor={id}>{t("diagnostics.logLevel")}</label>
        <Select
          id={id}
          value={settings.logLevel}
          options={LEVELS.map((level) => ({
            value: level,
            label: t(`diagnostics.levels.${level}`),
          }))}
          onChange={(level) => {
            update({ logLevel: level as LogLevel });
          }}
        />
      </div>
      <p className="cr-section-label cr-label">{t("diagnostics.currentLog")}</p>
      <p className="settings-hint cr-tight">{t("diagnostics.logHint")}</p>
      <div className="settings-actions cr-actions">
        <button
          type="button"
          className="settings-button cr-iconbtn"
          onClick={() => void copyLog(false)}
        >
          {t("diagnostics.copyTail")}
        </button>
        <button type="button" className="settings-button" onClick={() => void copyLog(true)}>
          {t("diagnostics.copyWhole")}
        </button>
        <button type="button" className="settings-button" onClick={() => void openLogFolder()}>
          {t("diagnostics.openLogFolder")}
        </button>
      </div>
      <p className="cr-section-label cr-label">{t("diagnostics.fileLabel")}</p>
      <div className="settings-actions cr-actions">
        <button type="button" className="settings-button" onClick={() => void save()}>
          {t("diagnostics.save")}
        </button>
      </div>
      <p role="status" className={status?.kind === "error" ? "settings-error" : "settings-success"}>
        {status?.text}
      </p>
    </Group>
  );
}
