import { useTranslation } from "react-i18next";
import type { Language } from "../../../api";
import { ENDONYMS, SUPPORTED_LANGUAGES } from "../../../i18n/resources";
import { RadioGroup } from "../components/RadioGroup";
import { Section } from "../components/Section";
import type { SettingsSectionProps } from "./types";

const SYSTEM = "system";
type Choice = Language | typeof SYSTEM;

// Sorted by the names people see, with a fixed collation so the order never jumps when the UI
// language changes.
const collator = new Intl.Collator("en");
const LANGUAGES = [...SUPPORTED_LANGUAGES].sort((a, b) =>
  collator.compare(ENDONYMS[a], ENDONYMS[b]),
);

/** Language: follow the OS or pick one of the 24 official EU languages (FR-1.36). */
export function LanguageSection({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Section title={t("sections.language")}>
      <RadioGroup<Choice>
        legend={t("language.legend")}
        value={settings.language ?? SYSTEM}
        options={[
          { value: SYSTEM, label: t("language.system") },
          ...LANGUAGES.map((value) => ({ value, label: ENDONYMS[value], lang: value })),
        ]}
        onChange={(choice) => {
          update({ language: choice === SYSTEM ? null : choice });
        }}
      />
    </Section>
  );
}
