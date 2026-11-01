import { useId } from "react";
import { useTranslation } from "react-i18next";
import { LanguagePicker } from "../../../components/LanguagePicker";
import { Group } from "../components/Section";
import type { SettingsSectionProps } from "./types";

/** Follow the OS or pick one of the 24 official EU languages (FR-1.36). */
export function LanguageCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  return (
    <Group title={t("language.title")} icon="globe">
      <div className="settings-control-row">
        <span className="settings-control-row__label">
          <label htmlFor={id}>{t("language.legend")}</label>
        </span>
        <LanguagePicker
          id={id}
          value={settings.language}
          onChange={(language) => {
            update({ language });
          }}
        />
      </div>
    </Group>
  );
}
