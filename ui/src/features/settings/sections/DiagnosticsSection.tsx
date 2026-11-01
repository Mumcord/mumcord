import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type LogLevel } from "../../../api";
import { Select } from "../../../components/Select";
import { Group } from "../components/Section";
import { errorText } from "../errors";
import { pickSavePath } from "../fileDialogs";
import type { SettingsSectionProps } from "./types";

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

  return (
    <Group title={t("diagnostics.title")} icon="file" info={t("diagnostics.fileHint")}>
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
      <div className="settings-actions">
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
