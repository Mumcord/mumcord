import { useTranslation } from "react-i18next";
import type { Language } from "../api";
import { LANGUAGE_OPTIONS } from "../i18n/languageOptions";
import { SUPPORTED_LANGUAGES } from "../i18n/resources";
import { SearchableSelect } from "./SearchableSelect";

const SYSTEM = "system";

// Sorted by the names people see, with a fixed collation so the order never jumps when the UI
// language changes.
function isLanguage(value: string): value is Language {
  return SUPPORTED_LANGUAGES.some((language) => language === value);
}

interface LanguagePickerProps {
  /** Id of the field, so a visible `<label htmlFor>` names it. */
  id: string;
  /** null follows the operating system. */
  value: Language | null;
  onChange: (language: Language | null) => void;
}

/** Searchable picker for the UI language: "Follow system" or one of the 24 EU languages (FR-1.36). */
export function LanguagePicker({ id, value, onChange }: LanguagePickerProps) {
  const { t } = useTranslation("settings");
  return (
    <SearchableSelect
      id={id}
      className="language-picker"
      value={value ?? SYSTEM}
      options={[{ value: SYSTEM, label: t("language.system") }, ...LANGUAGE_OPTIONS]}
      placeholder={t("language.search")}
      noMatch={t("language.noMatch")}
      onChange={(choice) => {
        onChange(isLanguage(choice) ? choice : null);
      }}
    />
  );
}
