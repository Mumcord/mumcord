import { useTranslation } from "react-i18next";
import type { Language } from "../../../api";
import { RadioGroup } from "../components/RadioGroup";
import { Section } from "../components/Section";
import type { SettingsSectionProps } from "./types";

const SYSTEM = "system";
type Choice = Language | typeof SYSTEM;

/** Language names are shown in their own language so everyone finds theirs. */
const ENDONYMS: Record<Language, string> = { en: "English", es: "Español", de: "Deutsch" };

/** Language: follow the OS or pick English, Spanish or German (FR-1.36). */
export function LanguageSection({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Section title={t("sections.language")}>
      <RadioGroup<Choice>
        legend={t("language.legend")}
        value={settings.language ?? SYSTEM}
        options={[
          { value: SYSTEM, label: t("language.system") },
          ...(Object.keys(ENDONYMS) as Language[]).map((value) => ({
            value,
            label: ENDONYMS[value],
          })),
        ]}
        onChange={(choice) => {
          update({ language: choice === SYSTEM ? null : choice });
        }}
      />
    </Section>
  );
}
