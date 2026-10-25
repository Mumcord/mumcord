import { useTranslation } from "react-i18next";
import "./betaBadge.css";

/** Small "Beta" pill (FR-1.73). Sighted users read "Beta"; screen readers hear "Beta feature". */
export function BetaBadge() {
  const { t } = useTranslation();
  return (
    <span className="beta-badge">
      <span aria-hidden="true">{t("beta")}</span>
      <span className="visually-hidden">{t("betaFeature")}</span>
    </span>
  );
}
