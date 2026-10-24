import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type LogLevel } from "../../../api";
import { Group, Section } from "../components/Section";
import { errorText } from "../errors";
import { pickSavePath } from "../fileDialogs";
import type { SettingsSectionProps } from "./types";

const LEVELS: LogLevel[] = ["error", "warn", "info", "debug"];

type Status = { kind: "success" | "error"; text: string } | null;

/** Diagnostics: log level and "Save diagnostic file" (FR-1.40). */
export function DiagnosticsSection({ settings, update }: SettingsSectionProps) {
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
    <Section title={t("sections.diagnostics")}>
      <Group title={t("diagnostics.logging")}>
        <div className="settings-field-row">
          <label className="settings-field" htmlFor={id}>
            {t("diagnostics.logLevel")}
          </label>
          <select
            id={id}
            className="settings-input"
            value={settings.logLevel}
            onChange={(event) => {
              update({ logLevel: event.target.value as LogLevel });
            }}
          >
            {LEVELS.map((level) => (
              <option key={level} value={level}>
                {t(`diagnostics.levels.${level}`)}
              </option>
            ))}
          </select>
        </div>
      </Group>
      <Group title={t("diagnostics.file")}>
        <p className="settings-hint">{t("diagnostics.fileHint")}</p>
        <button type="button" className="settings-button" onClick={() => void save()}>
          {t("diagnostics.save")}
        </button>
        <p
          role="status"
          className={status?.kind === "error" ? "settings-error" : "settings-success"}
        >
          {status?.text}
        </p>
      </Group>
    </Section>
  );
}
