import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import licenseText from "../../../../../LICENSE?raw";
import { getApi, type AppInfo } from "../../../api";
import { Group, Section } from "../components/Section";

/** About: version, OS, Mumcord's AGPL license and the third-party notices (AC-1.29). */
export function AboutSection() {
  const { t } = useTranslation("settings");
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [notices, setNotices] = useState<string | null>(null);
  const [noticesFailed, setNoticesFailed] = useState(false);

  useEffect(() => {
    let active = true;
    getApi()
      .appInfo()
      .then(
        (loaded) => {
          if (!active) return;
          setInfo(loaded);
          setNotices(loaded.thirdPartyNotices);
        },
        (error: unknown) => {
          console.error("Loading app info failed", error);
          if (active) setNoticesFailed(true);
        },
      );
    return () => {
      active = false;
    };
  }, []);

  return (
    <Section title={t("sections.about")}>
      <dl className="settings-facts">
        <dt>{t("about.version")}</dt>
        <dd>{info?.version ?? "…"}</dd>
        <dt>{t("about.os")}</dt>
        <dd>{info?.os ?? "…"}</dd>
      </dl>
      <Group title={t("about.license")}>
        <p className="settings-hint">{t("about.commercial")}</p>
        <ScrollableText label={t("about.license")} text={licenseText} />
      </Group>
      <Group title={t("about.notices")}>
        <p className="settings-hint">{t("about.noticesHint")}</p>
        {noticesFailed ? (
          <p role="alert" className="settings-error">
            {t("about.noticesFailed")}
          </p>
        ) : (
          <ScrollableText label={t("about.notices")} text={notices ?? t("about.loading")} />
        )}
      </Group>
    </Section>
  );
}

/** Long preformatted text in a scroll box; focusable so keyboard users can scroll it. */
function ScrollableText({ label, text }: { label: string; text: string }) {
  return (
    // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
    <pre className="settings-text" tabIndex={0} aria-label={label}>
      {text}
    </pre>
  );
}
