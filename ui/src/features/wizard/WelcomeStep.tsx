import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { Language } from "../../api";
import { Select } from "../../components/Select";
import { ENDONYMS, SUPPORTED_LANGUAGES } from "../../i18n/resources";
import type { SettingsSectionProps } from "../settings/sections/types";
import { StepFrame } from "./StepFrame";

const SYSTEM = "system";

// Sorted by the names people see, with a fixed collation so the order never jumps when the UI
// language changes (as in Settings → Language).
const collator = new Intl.Collator("en");
const LANGUAGES = [...SUPPORTED_LANGUAGES].sort((a, b) =>
  collator.compare(ENDONYMS[a], ENDONYMS[b]),
);

const POINTS = ["voice", "servers", "private"] as const;

function isLanguage(value: string): value is Language {
  return SUPPORTED_LANGUAGES.some((language) => language === value);
}

/** Welcome: what Mumcord is, and the UI language (applied at once, saved at Finish). */
export function WelcomeStep({
  settings,
  update,
  onNext,
}: SettingsSectionProps & { onNext: () => void }) {
  const { t } = useTranslation("wizard");
  const { t: ts } = useTranslation("settings");
  const languageId = useId();
  return (
    <StepFrame
      title={
        <>
          {t("welcome.title")}{" "}
          <span className="wizard-gradient-text">{t("welcome.titleAccent")}</span>
        </>
      }
      titleText={`${t("welcome.title")} ${t("welcome.titleAccent")}`}
      subtitle={t("welcome.subtitle")}
      badge={
        <img
          className="wizard-welcome__logo"
          src={`${import.meta.env.BASE_URL}favicon.png`}
          alt=""
          width={96}
          height={96}
        />
      }
      onSubmit={onNext}
    >
      <ul className="wizard-grid wizard-grid--three wizard-welcome__points">
        {POINTS.map((point) => (
          <li key={point} className="wizard-card">
            <p className="wizard-card__title">{t(`welcome.${point}.title`)}</p>
            <p className="wizard-muted wizard-small">{t(`welcome.${point}.text`)}</p>
          </li>
        ))}
      </ul>
      <div className="wizard-row">
        <label htmlFor={languageId}>{ts("language.legend")}</label>
        <Select
          id={languageId}
          value={settings.language ?? SYSTEM}
          options={[
            { value: SYSTEM, label: ts("language.system") },
            ...LANGUAGES.map((value) => ({ value, label: ENDONYMS[value] })),
          ]}
          onChange={(choice) => {
            update({ language: isLanguage(choice) ? choice : null });
          }}
        />
      </div>
    </StepFrame>
  );
}
