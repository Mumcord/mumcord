import { useState } from "react";
import { useTranslation } from "react-i18next";
import licenseText from "../../../../../LICENSE?raw";
import { announce } from "../../../a11y";
import { getApi } from "../../../api";
import { useUnlockClicks } from "../../inspector/devTools";
import { openInBrowser } from "../../chat/openLink";
import { useView } from "../../../app/view";
import { openSettingsSection } from "../settingsOpener";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import { useAppInfo } from "../useAppInfo";
import type { SettingsSectionProps } from "./types";

const ISSUES_URL = "https://github.com/Mumcord/mumcord/issues/new";

/** Version, OS, What's new and Mumcord's AGPL license (AC-1.29). */
export function VersionCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const info = useAppInfo();
  const [changelogFailed, setChangelogFailed] = useState(false);
  const [unlocked, setUnlocked] = useState(false);
  const versionClicked = useUnlockClicks(() => {
    setUnlocked(true);
    announce(t("about.devToolsUnlocked"));
  });

  return (
    <Group title={t("about.versionTitle")} icon="info" wide>
      <dl className="settings-facts">
        <dt>{t("about.version")}</dt>
        {/* Clicking the version seven times unlocks the developer tools (issue #60); the
            gesture is deliberately hidden, so the text is not announced as a button. */}
        {/* eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-noninteractive-element-interactions */}
        <dd className="settings-version" onClick={versionClicked}>
          {info?.version ?? "…"}
          {info?.portable === true && ` · ${t("about.portable")}`}
        </dd>
        <dt>{t("about.os")}</dt>
        <dd>{info?.os ?? "…"}</dd>
      </dl>
      {unlocked && (
        <p role="status" className="settings-success">
          {t("about.devToolsUnlocked")}
        </p>
      )}
      <div className="settings-actions">
        <button
          type="button"
          className="settings-button"
          onClick={() => {
            setChangelogFailed(false);
            getApi()
              .changelogOpen()
              .catch((error: unknown) => {
                console.error("Opening the changelog window failed", error);
                setChangelogFailed(true);
              });
          }}
        >
          {t("about.whatsNew")}
        </button>
      </div>
      {changelogFailed && (
        <p role="alert" className="settings-error">
          {t("about.changelogFailed")}
        </p>
      )}
      <Switch
        label={t("about.whatsNewAuto")}
        checked={settings.whatsNewAuto}
        onChange={(on) => {
          update({ whatsNewAuto: on });
        }}
      />
      <ScrollableText label={t("about.license")} text={licenseText} />
    </Group>
  );
}

/** Running the setup wizard again (#149); identities are managed in Account & Identity. */
export function SetupCard() {
  const { t } = useTranslation("settings");
  const { openRerunWizard } = useView();
  return (
    <Group title={t("about.setup")} icon="star" info={t("about.setupWizardHint")}>
      <div className="settings-actions">
        <button type="button" className="settings-button" onClick={openRerunWizard}>
          {t("about.setupWizardRun")}
        </button>
      </div>
      <p className="settings-hint">
        {t("about.setupWizardIdentityNote")}{" "}
        <button
          type="button"
          className="settings-link"
          onClick={() => {
            openSettingsSection("account");
          }}
        >
          {t("about.setupWizardIdentityLink")}
        </button>
      </p>
    </Group>
  );
}

/** Bug reports and feature requests on GitHub. */
export function SupportCard() {
  const { t } = useTranslation("settings");
  return (
    <Group title={t("about.support")} icon="lifeBuoy">
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
  );
}

/** Checking GitHub for a newer version. */
export function UpdatesCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Group title={t("updates.title")} icon="refresh">
      <Switch
        label={t("updates.check")}
        description={t("updates.hint")}
        checked={settings.updateCheck === "yes"}
        onChange={(on) => {
          update({ updateCheck: on ? "yes" : "no" });
        }}
      />
    </Group>
  );
}

/** The third-party licences window (#138). */
export function LicencesCard() {
  const { t } = useTranslation("settings");
  const [failed, setFailed] = useState(false);
  return (
    <Group title={t("about.notices")} icon="file" info={t("about.noticesHint")}>
      <div className="settings-actions">
        <button
          type="button"
          className="settings-button"
          onClick={() => {
            setFailed(false);
            getApi()
              .licencesOpen()
              .catch((error: unknown) => {
                console.error("Opening the licences window failed", error);
                setFailed(true);
              });
          }}
        >
          {t("about.showNotices")}
        </button>
      </div>
      {failed && (
        <p role="alert" className="settings-error">
          {t("about.noticesFailed")}
        </p>
      )}
    </Group>
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
