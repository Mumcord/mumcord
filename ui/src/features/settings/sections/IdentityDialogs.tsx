import { useId, useState, type SubmitEvent } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type Identity, type SavedServer } from "../../../api";
import { errorText } from "../errors";
import { ModalDialog } from "../components/ModalDialog";
import { WarningIcon } from "../components/WarningIcon";
import { fullFingerprint, serversFor } from "../identityFormat";

/** Identity whose key material the dialogs act on; everything else comes from the section. */
interface IdentityDialogProps {
  identity: Identity;
  onCancel: () => void;
  onDone: () => void;
}

const NAME_MAX = 64;

/** "New identity" (FR-3.5): certificate name plus a local label, both required. */
export function CreateIdentityDialog({ onCancel, onDone }: Omit<IdentityDialogProps, "identity">) {
  const { t } = useTranslation("settings");
  const id = useId();
  const [name, setName] = useState("");
  const [label, setLabel] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (busy) return;
    if (name.trim() === "") {
      setProblem(t("identity.nameRequired"));
      return;
    }
    if (label.trim() === "") {
      setProblem(t("identity.labelRequired"));
      return;
    }
    setProblem(null);
    setBusy(true);
    try {
      await getApi().identityCreate(name.trim(), label.trim());
      onDone();
    } catch (error: unknown) {
      setBusy(false);
      setProblem(errorText(t, error));
    }
  };

  return (
    <ModalDialog title={t("identity.createTitle")} onClose={onCancel}>
      <form onSubmit={(event) => void submit(event)}>
        <p>{t("identity.createIntro")}</p>
        <label className="settings-field" htmlFor={`${id}-name`}>
          {t("identity.name")}
        </label>
        <input
          id={`${id}-name`}
          className="settings-input"
          value={name}
          maxLength={NAME_MAX}
          autoComplete="off"
          aria-describedby={`${id}-name-hint`}
          onChange={(event) => {
            setName(event.target.value);
            if (label === "") setLabel(event.target.value);
          }}
        />
        <p id={`${id}-name-hint`} className="settings-hint">
          {t("identity.nameHint")}
        </p>
        <label className="settings-field" htmlFor={`${id}-label`}>
          {t("identity.label")}
        </label>
        <input
          id={`${id}-label`}
          className="settings-input"
          value={label}
          maxLength={40}
          autoComplete="off"
          aria-describedby={`${id}-label-hint`}
          onChange={(event) => {
            setLabel(event.target.value);
          }}
        />
        <p id={`${id}-label-hint`} className="settings-hint">
          {t("identity.labelHint")}
        </p>
        <div role="alert" className="settings-error">
          {problem}
        </div>
        <div className="settings-dialog__actions">
          <button type="button" className="settings-button" onClick={onCancel}>
            {t("cancel", { ns: "common" })}
          </button>
          <button
            type="submit"
            className="settings-button settings-button--primary"
            disabled={busy}
          >
            {t("identity.create")}
          </button>
        </div>
      </form>
    </ModalDialog>
  );
}

/** Import (FR-3.6): password of the PKCS#12 file plus the label it gets in the list. */
export function ImportIdentityDialog({
  path,
  defaultLabel,
  onCancel,
  onDone,
}: {
  path: string;
  defaultLabel: string;
  onCancel: () => void;
  onDone: (identity: Identity) => void;
}) {
  const { t } = useTranslation("settings");
  const id = useId();
  const [password, setPassword] = useState("");
  const [label, setLabel] = useState(defaultLabel);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // Left untouched, the label stays the engine's default: the certificate's common name (AC-3.3).
  const [edited, setEdited] = useState(false);

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (busy) return;
    if (label.trim() === "") {
      setProblem(t("identity.labelRequired"));
      return;
    }
    setProblem(null);
    setBusy(true);
    try {
      const imported = await getApi().identityImport(
        path,
        password === "" ? null : password,
        edited ? label.trim() : null,
      );
      onDone(imported);
    } catch (error: unknown) {
      setBusy(false);
      setProblem(errorText(t, error));
    }
  };

  return (
    <ModalDialog title={t("identity.importTitle")} onClose={onCancel}>
      <form onSubmit={(event) => void submit(event)}>
        <p>{t("identity.importIntro")}</p>
        <label className="settings-field" htmlFor={`${id}-pw`}>
          {t("identity.importPassword")}
        </label>
        <input
          id={`${id}-pw`}
          type="password"
          className="settings-input"
          autoComplete="off"
          value={password}
          onChange={(event) => {
            setPassword(event.target.value);
          }}
        />
        <label className="settings-field" htmlFor={`${id}-label`}>
          {t("identity.label")}
        </label>
        <input
          id={`${id}-label`}
          className="settings-input"
          value={label}
          maxLength={40}
          autoComplete="off"
          onChange={(event) => {
            setLabel(event.target.value);
            setEdited(true);
          }}
        />
        <div role="alert" className="settings-error">
          {problem}
        </div>
        <div className="settings-dialog__actions">
          <button type="button" className="settings-button" onClick={onCancel}>
            {t("cancel", { ns: "common" })}
          </button>
          <button
            type="submit"
            className="settings-button settings-button--primary"
            disabled={busy}
          >
            {t("identity.import")}
          </button>
        </div>
      </form>
    </ModalDialog>
  );
}

/** Rename (FR-3.7): local label only, the certificate stays the same. */
export function RenameIdentityDialog({ identity, onCancel, onDone }: IdentityDialogProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  const [label, setLabel] = useState(identity.label);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (busy) return;
    if (label.trim() === "") {
      setProblem(t("identity.labelRequired"));
      return;
    }
    setProblem(null);
    setBusy(true);
    try {
      await getApi().identityRename(identity.id, label.trim());
      onDone();
    } catch (error: unknown) {
      setBusy(false);
      setProblem(errorText(t, error));
    }
  };

  return (
    <ModalDialog title={t("identity.renameTitle", { identity: identity.label })} onClose={onCancel}>
      <form onSubmit={(event) => void submit(event)}>
        <p>{t("identity.renameIntro")}</p>
        <label className="settings-field" htmlFor={`${id}-label`}>
          {t("identity.label")}
        </label>
        <input
          id={`${id}-label`}
          className="settings-input"
          value={label}
          maxLength={40}
          autoComplete="off"
          onChange={(event) => {
            setLabel(event.target.value);
          }}
        />
        <div role="alert" className="settings-error">
          {problem}
        </div>
        <div className="settings-dialog__actions">
          <button type="button" className="settings-button" onClick={onCancel}>
            {t("cancel", { ns: "common" })}
          </button>
          <button
            type="submit"
            className="settings-button settings-button--primary"
            disabled={busy}
          >
            {t("save", { ns: "common" })}
          </button>
        </div>
      </form>
    </ModalDialog>
  );
}

/** Details (FR-3.4): fingerprint, certificate name, date, default state and the servers. */
export function IdentityDetailsDialog({
  identity,
  servers,
  onCancel,
}: {
  identity: Identity;
  servers: readonly SavedServer[];
  onCancel: () => void;
}) {
  const { t, i18n } = useTranslation("settings");
  const usedBy = serversFor(identity, servers).map((server) => server.label || server.address);
  const created = new Intl.DateTimeFormat(i18n.language, { dateStyle: "long" }).format(
    new Date(identity.addedAt),
  );
  return (
    <ModalDialog
      title={t("identity.detailsTitle", { identity: identity.label })}
      onClose={onCancel}
    >
      <dl className="settings-details">
        <dt>{t("identity.fingerprint")}</dt>
        <dd className="settings-details__fingerprint">{fullFingerprint(identity.certHash)}</dd>
        <dt>{t("identity.certName")}</dt>
        <dd>{identity.name}</dd>
        <dt>{t("identity.created")}</dt>
        <dd>{created}</dd>
        <dt>{t("identity.usedByLabel")}</dt>
        <dd>{usedBy.length === 0 ? t("identity.notUsed") : usedBy.join(", ")}</dd>
      </dl>
      {identity.isDefault && <p className="identity-card__badge">{t("identity.defaultBadge")}</p>}
      <div className="settings-dialog__actions">
        <button type="button" className="settings-button" onClick={onCancel}>
          {t("close", { ns: "common" })}
        </button>
      </div>
    </ModalDialog>
  );
}

/**
 * Delete (FR-3.9): strong warning, the servers that fall back to the default, and a typed
 * confirmation. `onBackup` starts the export flow for this identity.
 */
export function DeleteIdentityDialog({
  identity,
  identities,
  servers,
  onBackup,
  onCancel,
  onDone,
}: IdentityDialogProps & {
  identities: readonly Identity[];
  servers: readonly SavedServer[];
  onBackup: (identity: Identity) => void;
}) {
  const { t } = useTranslation("settings");
  const id = useId();
  const [typed, setTyped] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const used = serversFor(identity, servers);
  const remaining = identities.filter((other) => other.id !== identity.id);
  const nextDefault = remaining.find((other) => other.isDefault) ?? remaining[0];
  const last = remaining.length === 0;
  const confirmed = typed.trim() === identity.label;

  const remove = async () => {
    if (!confirmed || busy) return;
    setBusy(true);
    try {
      await getApi().identityDelete(identity.id);
      onDone();
    } catch (error: unknown) {
      setBusy(false);
      setProblem(errorText(t, error));
    }
  };

  return (
    <ModalDialog title={t("identity.deleteTitle", { identity: identity.label })} onClose={onCancel}>
      <p className="identity-warning" role="alert">
        <WarningIcon className="identity-warning__icon" />
        {t("identity.deleteWarning")}
      </p>
      {used.length > 0 && (
        <p className="settings-hint">
          {t("identity.deleteUsedBy", {
            servers: used.map((server) => server.label || server.address).join(", "),
            identity: nextDefault?.label ?? "",
          })}
        </p>
      )}
      {identity.isDefault && !last && nextDefault && (
        <p className="settings-hint">
          {t("identity.deleteNewDefault", { identity: nextDefault.label })}
        </p>
      )}
      {last ? (
        <p className="settings-hint">{t("identity.lastIdentity")}</p>
      ) : (
        <>
          <button
            type="button"
            className="settings-button"
            onClick={() => {
              onBackup(identity);
            }}
          >
            {t("identity.backupFirst")}
          </button>
          <label className="settings-field" htmlFor={`${id}-confirm`}>
            {t("identity.typeToConfirm")}
          </label>
          <input
            id={`${id}-confirm`}
            className="settings-input"
            value={typed}
            autoComplete="off"
            aria-describedby={`${id}-confirm-hint`}
            onChange={(event) => {
              setTyped(event.target.value);
            }}
          />
          <p id={`${id}-confirm-hint`} className="settings-hint">
            {t("identity.typeToConfirmHint", { identity: identity.label })}
          </p>
        </>
      )}
      <div role="alert" className="settings-error">
        {problem}
      </div>
      <div className="settings-dialog__actions">
        <button type="button" className="settings-button" onClick={onCancel}>
          {t("cancel", { ns: "common" })}
        </button>
        <button
          type="button"
          className="settings-button settings-button--danger"
          disabled={!confirmed || last || busy}
          aria-disabled={!confirmed || last || busy}
          onClick={() => void remove()}
        >
          {t("identity.delete")}
        </button>
      </div>
    </ModalDialog>
  );
}
