import { useTranslation } from "react-i18next";
import { Section } from "../components/Section";

/** Overlay & positional audio arrive in stage 1b; until then the section only says so. */
export function OverlaySection() {
  const { t } = useTranslation("settings");
  return (
    <Section title={t("sections.overlay")}>
      <p className="settings-hint">{t("overlay.comingLater")}</p>
    </Section>
  );
}
