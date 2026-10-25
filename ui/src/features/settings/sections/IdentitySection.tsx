import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type Identity } from "../../../api";
import { PasswordDialog } from "../components/PasswordDialog";
import { Group, Section } from "../components/Section";
import { errorText } from "../errors";
import { pickOpenPath, pickSavePath } from "../fileDialogs";
import { DefaultAvatarGroup } from "./DefaultAvatarGroup";
import { NowGamingGroup } from "./NowGamingGroup";
import { NowPlayingGroup } from "./NowPlayingGroup";
import type { SettingsSectionProps } from "./types";

const IDENTITY_FILTER_EXTENSIONS = ["p12", "pfx"];

type Status = { kind: "success" | "error"; text: string } | null;

/**
 * Identity: shows the name, offers export/import of the PKCS#12 identity (FR-1.3) and the
 * profile's default avatar (FR-1.47).
 */
export function IdentitySection({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const [identity, setIdentity] = useState<Identity | null>(null);
  const [exportPath, setExportPath] = useState<string | null>(null);
  const [importPath, setImportPath] = useState<string | null>(null);
  const [status, setStatus] = useState<Status>(null);

  useEffect(() => {
    let active = true;
    getApi()
      .identityGet()
      .then(
        (loaded) => {
          if (active) setIdentity(loaded);
        },
        (error: unknown) => {
          console.error("Loading the identity failed", error);
          if (active) setStatus({ kind: "error", text: errorText(t, error) });
        },
      );
    return () => {
      active = false;
    };
  }, [t]);

  const filter = { name: t("identity.fileType"), extensions: IDENTITY_FILTER_EXTENSIONS };

  const describe = (error: unknown) => {
    console.error("Identity export/import failed", error);
    return errorText(t, error);
  };

  const startExport = async () => {
    setStatus(null);
    try {
      const path = await pickSavePath("mumcord-identity.p12", filter, t("identity.exportTitle"));
      if (path !== null) setExportPath(path);
    } catch (error: unknown) {
      setStatus({ kind: "error", text: describe(error) });
    }
  };

  const startImport = async () => {
    setStatus(null);
    try {
      const path = await pickOpenPath(filter, t("identity.importTitle"));
      if (path !== null) setImportPath(path);
    } catch (error: unknown) {
      setStatus({ kind: "error", text: describe(error) });
    }
  };

  return (
    <Section title={t("sections.identity")}>
      <Group title={t("identity.current")}>
        <p className="settings-value">{identity?.name ?? t("identity.none")}</p>
        <p className="settings-hint">{t("identity.explanation")}</p>
      </Group>
      <Group title={t("identity.backup")}>
        <p className="settings-hint">{t("identity.exportHint")}</p>
        <div className="settings-actions">
          <button type="button" className="settings-button" onClick={() => void startExport()}>
            {t("identity.export")}
          </button>
          <button type="button" className="settings-button" onClick={() => void startImport()}>
            {t("identity.import")}
          </button>
        </div>
      </Group>
      <DefaultAvatarGroup settings={settings} update={update} />
      <NowPlayingGroup settings={settings} update={update} />
      <NowGamingGroup settings={settings} update={update} />
      <p role="status" className={status?.kind === "error" ? "settings-error" : "settings-success"}>
        {status?.text}
      </p>
      {exportPath !== null && (
        <PasswordDialog
          title={t("identity.exportTitle")}
          message={t("identity.exportPasswordHint")}
          passwordLabel={t("identity.password")}
          confirmLabel={t("identity.exportConfirm")}
          confirmTwice
          describeError={describe}
          onSubmit={async (password) => {
            await getApi().identityExport(exportPath, password);
            setExportPath(null);
            setStatus({ kind: "success", text: t("identity.exported") });
          }}
          onCancel={() => {
            setExportPath(null);
          }}
        />
      )}
      {importPath !== null && (
        <PasswordDialog
          title={t("identity.importTitle")}
          message={t("identity.importWarning")}
          passwordLabel={t("identity.importPassword")}
          confirmLabel={t("identity.importConfirm")}
          danger
          describeError={describe}
          onSubmit={async (password) => {
            const imported = await getApi().identityImport(
              importPath,
              password === "" ? null : password,
            );
            setImportPath(null);
            setIdentity(imported);
            setStatus({ kind: "success", text: t("identity.imported", { name: imported.name }) });
          }}
          onCancel={() => {
            setImportPath(null);
          }}
        />
      )}
    </Section>
  );
}
