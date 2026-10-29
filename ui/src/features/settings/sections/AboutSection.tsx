import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import licenseText from "../../../../../LICENSE?raw";
import { announce } from "../../../a11y";
import { getApi, type AppInfo } from "../../../api";
import { useUnlockClicks } from "../../inspector/devTools";
import { openInBrowser } from "../../chat/openLink";
import { Group, Section } from "../components/Section";
import { Switch } from "../components/Switch";
import { AppImageMenuEntrySwitch } from "./AppImageMenuEntrySwitch";
import type { SettingsSectionProps } from "./types";

const ISSUES_URL = "https://github.com/Mumcord/mumcord/issues/new";

/** About: version, OS, Mumcord's AGPL license and the third-party notices (AC-1.29). */
export function AboutSection({ settings, update }: SettingsSectionProps) {
  const { t, i18n } = useTranslation("settings");
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [noticesFailed, setNoticesFailed] = useState(false);
  const [unlocked, setUnlocked] = useState(false);
  const versionClicked = useUnlockClicks(() => {
    setUnlocked(true);
    announce(t("about.devToolsUnlocked"));
  });

  useEffect(() => {
    let active = true;
    getApi()
      .appInfo()
      .then(
        (loaded) => {
          if (active) setInfo(loaded);
        },
        (error: unknown) => {
          console.error("Loading app info failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);

  const changelogBase = i18n.language.startsWith("de")
    ? "https://mumcord.app/changelog"
    : "https://mumcord.app/en/changelog";

  return (
    <Section title={t("sections.about")}>
      <dl className="settings-facts">
        <dt>{t("about.version")}</dt>
        {/* Clicking the version seven times unlocks the developer tools (issue #60); the
            gesture is deliberately hidden, so the text is not announced as a button. */}
        {/* eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-noninteractive-element-interactions */}
        <dd className="settings-version" onClick={versionClicked}>
          {info?.version ?? "…"}
        </dd>
        <dt>{t("about.os")}</dt>
        <dd>{info?.os ?? "…"}</dd>
      </dl>
      {unlocked && (
        <p role="status" className="settings-success">
          {t("about.devToolsUnlocked")}
        </p>
      )}
      {info && (
        <div className="settings-group">
          <button
            type="button"
            className="settings-button"
            onClick={() => {
              // Dev builds have no changelog entry of their own; they open their base release.
              openInBrowser(`${changelogBase}#v${info.packageVersion}`);
            }}
          >
            {t("about.whatsNew")}
          </button>
        </div>
      )}
      <Group title={t("updates.title")}>
        <Switch
          label={t("updates.check")}
          description={t("updates.hint")}
          checked={settings.updateCheck === "yes"}
          onChange={(on) => {
            update({ updateCheck: on ? "yes" : "no" });
          }}
        />
        {info?.appimage === true && <AppImageMenuEntrySwitch />}
      </Group>
      <Group title={t("about.support")}>
        <div className="settings-actions">
          <button
            type="button"
            className="settings-button"
            onClick={() => {
              openInBrowser(`${ISSUES_URL}?template=bug_report.yml`);
            }}
          >
            {t("about.reportBug")}
          </button>
          <button
            type="button"
            className="settings-button"
            onClick={() => {
              openInBrowser(`${ISSUES_URL}?template=feature_request.yml`);
            }}
          >
            {t("about.requestFeature")}
          </button>
        </div>
      </Group>
      <Group title={t("about.license")}>
        <ScrollableText label={t("about.license")} text={licenseText} />
      </Group>
      <Group title={t("about.notices")}>
        <p className="settings-hint">{t("about.noticesHint")}</p>
        <div className="settings-group">
          <button
            type="button"
            className="settings-button"
            onClick={() => {
              setNoticesFailed(false);
              getApi()
                .licencesOpen()
                .catch((error: unknown) => {
                  console.error("Opening the licences window failed", error);
                  setNoticesFailed(true);
                });
            }}
          >
            {t("about.showNotices")}
          </button>
        </div>
        {noticesFailed && (
          <p role="alert" className="settings-error">
            {t("about.noticesFailed")}
          </p>
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
