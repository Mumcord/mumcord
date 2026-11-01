import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type Identity } from "../../../api";
import { PasswordDialog } from "../components/PasswordDialog";
import { WarningIcon } from "../components/WarningIcon";
import { Group } from "../components/Section";
import { errorText } from "../errors";
import { pickOpenPath, pickSavePath } from "../fileDialogs";
import { refreshIdentities, useServers } from "../../servers/store";
import {
  CreateIdentityDialog,
  DeleteIdentityDialog,
  IdentityDetailsDialog,
  ImportIdentityDialog,
  RenameIdentityDialog,
} from "./IdentityDialogs";
import { identityInitial, identityTone, serversFor, shortFingerprint } from "../identityFormat";

const IDENTITY_FILTER_EXTENSIONS = ["p12", "pfx"];

type Dialog =
  | { kind: "create" }
  | { kind: "import"; path: string; defaultLabel: string }
  | { kind: "rename"; identity: Identity }
  | { kind: "details"; identity: Identity }
  | { kind: "delete"; identity: Identity }
  | null;

type Status = { kind: "success" | "error"; text: string } | null;

/**
 * The identity manager (issue #148): the list of client certificates with create, import,
 * rename, export, details, delete and "set as default". Certificates and keys stay in the OS
 * secure storage; this list shows local metadata only.
 */
export function IdentitiesCard() {
  const { t, i18n } = useTranslation("settings");
  const { identities, servers } = useServers();
  const [dialog, setDialog] = useState<Dialog>(null);
  const [status, setStatus] = useState<Status>(null);
  const [exportPath, setExportPath] = useState<{ id: number; path: string } | null>(null);

  useEffect(() => {
    void refreshIdentities();
  }, []);

  const done = () => {
    setDialog(null);
    void refreshIdentities();
  };

  const describe = (error: unknown) => {
    console.error("An identity action failed", error);
    return errorText(t, error);
  };

  const startExport = async (identity: Identity) => {
    setStatus(null);
    try {
      const path = await pickSavePath(
        `mumcord-identity-${identity.label.replace(/[^\w.-]+/g, "-")}.p12`,
        { name: t("identity.fileType"), extensions: IDENTITY_FILTER_EXTENSIONS },
        t("identity.exportTitle", { identity: identity.label }),
      );
      if (path !== null) setExportPath({ id: identity.id, path });
    } catch (error: unknown) {
      setStatus({ kind: "error", text: describe(error) });
    }
  };

  const startImport = async () => {
    setStatus(null);
    try {
      const path = await pickOpenPath(
        { name: t("identity.fileType"), extensions: IDENTITY_FILTER_EXTENSIONS },
        t("identity.importTitle"),
      );
      if (path === null) return;
      const file = path.split(/[\\/]/).pop() ?? "";
      setDialog({
        kind: "import",
        path,
        defaultLabel: file.replace(/\.p12$|\.pfx$/i, "") || t("identity.importedLabel"),
      });
    } catch (error: unknown) {
      setStatus({ kind: "error", text: describe(error) });
    }
  };

  const setDefault = async (identity: Identity) => {
    setStatus(null);
    try {
      await getApi().identitySetDefault(identity.id);
      await refreshIdentities();
      setStatus({ kind: "success", text: t("identity.defaultSet", { identity: identity.label }) });
    } catch (error: unknown) {
      setStatus({ kind: "error", text: describe(error) });
    }
  };

  const created = new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium" });

  return (
    <Group
      title={t("identity.listTitle")}
      icon="key"
      info={t("identity.intro")}
      wide
      actions={
        <>
          <button type="button" className="settings-button" onClick={() => void startImport()}>
            {t("identity.import")}
          </button>
          <button
            type="button"
            className="settings-button settings-button--primary"
            onClick={() => {
              setDialog({ kind: "create" });
            }}
          >
            {t("identity.new")}
          </button>
        </>
      }
    >
      <ul className="identity-list" aria-label={t("identity.listLabel")}>
        {identities.map((identity) => {
          const used = serversFor(identity, servers);
          return (
            <li key={identity.id} className="identity-card">
              <span
                className={`identity-card__avatar ${identityTone(identity.id)}`}
                aria-hidden="true"
              >
                {identityInitial(identity.label)}
              </span>
              <div className="identity-card__body">
                <p className="identity-card__title">
                  <strong>{identity.label}</strong>
                  {identity.isDefault && (
                    <span className="identity-card__badge">{t("identity.defaultBadge")}</span>
                  )}
                </p>
                <p className="identity-card__meta">
                  {t("identity.createdLine", {
                    name: identity.name,
                    date: created.format(new Date(identity.addedAt)),
                  })}
                </p>
              </div>
              <div className="identity-card__actions">
                {!identity.isDefault && (
                  <button
                    type="button"
                    className="settings-button"
                    aria-label={t("identity.setDefaultFor", { identity: identity.label })}
                    onClick={() => void setDefault(identity)}
                  >
                    {t("identity.setDefault")}
                  </button>
                )}
                <button
                  type="button"
                  className="settings-button"
                  aria-label={t("identity.detailsFor", { identity: identity.label })}
                  onClick={() => {
                    setDialog({ kind: "details", identity });
                  }}
                >
                  {t("identity.details")}
                </button>
                <button
                  type="button"
                  className="settings-button"
                  aria-label={t("identity.renameFor", { identity: identity.label })}
                  onClick={() => {
                    setDialog({ kind: "rename", identity });
                  }}
                >
                  {t("identity.rename")}
                </button>
                <button
                  type="button"
                  className="settings-button"
                  aria-label={t("identity.exportFor", { identity: identity.label })}
                  onClick={() => void startExport(identity)}
                >
                  {t("identity.export")}
                </button>
                <button
                  type="button"
                  className="settings-button settings-button--danger"
                  aria-label={t("identity.deleteFor", { identity: identity.label })}
                  onClick={() => {
                    setDialog({ kind: "delete", identity });
                  }}
                >
                  {t("identity.delete")}
                </button>
              </div>
              <p className="identity-card__meta identity-card__fingerprint">
                {t("identity.fingerprintLine", {
                  fingerprint: shortFingerprint(identity.certHash),
                })}
                {" · "}
                {used.length === 0
                  ? t("identity.notUsed")
                  : t("identity.usedBy", {
                      servers: used.map((server) => server.label || server.address).join(", "),
                    })}
              </p>
            </li>
          );
        })}
      </ul>
      <p className="identity-warning" role="note">
        <WarningIcon className="identity-warning__icon" />
        {t("identity.backupWarning")}
      </p>
      <p role="status" className={status?.kind === "error" ? "settings-error" : "settings-success"}>
        {status?.text}
      </p>
      {dialog?.kind === "create" && (
        <CreateIdentityDialog
          onCancel={() => {
            setDialog(null);
          }}
          onDone={done}
        />
      )}
      {dialog?.kind === "import" && (
        <ImportIdentityDialog
          path={dialog.path}
          defaultLabel={dialog.defaultLabel}
          onCancel={() => {
            setDialog(null);
          }}
          onDone={(imported) => {
            done();
            setStatus({
              kind: "success",
              text: t("identity.imported", { name: imported.label }),
            });
          }}
        />
      )}
      {dialog?.kind === "rename" && (
        <RenameIdentityDialog
          identity={dialog.identity}
          onCancel={() => {
            setDialog(null);
          }}
          onDone={done}
        />
      )}
      {dialog?.kind === "details" && (
        <IdentityDetailsDialog
          identity={dialog.identity}
          servers={servers}
          onCancel={() => {
            setDialog(null);
          }}
        />
      )}
      {dialog?.kind === "delete" && (
        <DeleteIdentityDialog
          identity={dialog.identity}
          identities={identities}
          servers={servers}
          onBackup={(identity) => void startExport(identity)}
          onCancel={() => {
            setDialog(null);
          }}
          onDone={() => {
            done();
            setStatus({ kind: "success", text: t("identity.deleted") });
          }}
        />
      )}
      {exportPath !== null && (
        <PasswordDialog
          title={t("identity.exportTitle", {
            identity: identities.find((i) => i.id === exportPath.id)?.label ?? "",
          })}
          message={t("identity.exportPasswordHint")}
          passwordLabel={t("identity.password")}
          confirmLabel={t("identity.exportConfirm")}
          confirmTwice
          describeError={describe}
          onSubmit={async (password) => {
            await getApi().identityExport(exportPath.id, exportPath.path, password);
            setExportPath(null);
            setStatus({ kind: "success", text: t("identity.exported") });
          }}
          onCancel={() => {
            setExportPath(null);
          }}
        />
      )}
    </Group>
  );
}
