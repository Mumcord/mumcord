import { useEffect, useId, useRef, useState, type SyntheticEvent } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type RepositoryPreview } from "../../../api";
import { ModalDialog } from "../../settings/components/ModalDialog";
import { repositoryProblem } from "./describeError";

interface AddRepositoryDialogProps {
  /** Pre-filled address and "Expected key" when a server suggested the repository. */
  initialUrl?: string;
  initialKey?: string | null;
  onAdded: () => void;
  onCancel: () => void;
}

/**
 * Two steps (FR-6.74): look at the repository first (name, homepage, contact, key fingerprint),
 * then add it. The engine fetches and verifies everything; texts from the repository are shown
 * as plain text only.
 */
export function AddRepositoryDialog({
  initialUrl = "",
  initialKey = null,
  onAdded,
  onCancel,
}: AddRepositoryDialogProps) {
  const { t } = useTranslation(["pluginRepos", "common"]);
  const id = useId();
  const [url, setUrl] = useState(initialUrl);
  const [key, setKey] = useState(initialKey ?? "");
  const [preview, setPreview] = useState<RepositoryPreview | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const cancelRef = useRef<HTMLButtonElement>(null);

  // Cancel is the safe default once the repository is shown (FR-6.74), so Enter never adds it.
  useEffect(() => {
    if (preview !== null) cancelRef.current?.focus();
  }, [preview]);

  const expected = key.trim() === "" ? null : key.trim();

  const check = async (event: SyntheticEvent) => {
    event.preventDefault();
    setBusy(true);
    setProblem(null);
    try {
      setPreview(await getApi().pluginRepoPreview(url.trim(), expected));
    } catch (error: unknown) {
      setPreview(null);
      setProblem(repositoryProblem(error));
    } finally {
      setBusy(false);
    }
  };

  const add = async () => {
    setBusy(true);
    setProblem(null);
    try {
      await getApi().pluginRepoAdd(url.trim(), expected);
      onAdded();
    } catch (error: unknown) {
      setProblem(repositoryProblem(error));
      setBusy(false);
    }
  };

  return (
    <ModalDialog title={t("add.title")} onClose={onCancel}>
      <p className="plugin-repos__subtitle">{t("add.subtitle")}</p>
      <form onSubmit={(event) => void check(event)} className="plugin-repos__form">
        <label htmlFor={`${id}-url`} className="plugin-repos__label">
          {t("add.address")}
        </label>
        <input
          id={`${id}-url`}
          className="settings-input plugin-repos__mono"
          type="url"
          inputMode="url"
          autoComplete="off"
          spellCheck={false}
          placeholder="https://"
          value={url}
          onChange={(event) => {
            setUrl(event.target.value);
            setPreview(null);
          }}
        />
        <label htmlFor={`${id}-key`} className="plugin-repos__label">
          {t("add.expectedKey")}
        </label>
        <input
          id={`${id}-key`}
          className="settings-input plugin-repos__mono"
          autoComplete="off"
          spellCheck={false}
          aria-describedby={`${id}-key-hint`}
          value={key}
          onChange={(event) => {
            setKey(event.target.value);
            setPreview(null);
          }}
        />
        <p id={`${id}-key-hint`} className="plugin-repos__hint">
          {t("add.expectedKeyHint")}
        </p>
        {preview === null && (
          <div className="settings-dialog__actions">
            <button
              type="button"
              className="settings-button settings-button--primary"
              onClick={onCancel}
            >
              {t("common:cancel")}
            </button>
            <button
              type="submit"
              className="settings-button settings-button--primary"
              disabled={busy || url.trim() === ""}
            >
              {t("add.check")}
            </button>
          </div>
        )}
      </form>
      {problem !== null && (
        <p role="alert" className="plugin-repos__problem">
          {problem}
        </p>
      )}
      {preview !== null && (
        <>
          <div className="plugin-repos__card" role="group" aria-label={t("add.found")}>
            <span className="plugin-repos__card-icon" aria-hidden="true">
              {preview.name.trim().charAt(0).toUpperCase()}
            </span>
            <div className="plugin-repos__card-body">
              <strong>{preview.name}</strong>
              <span className="plugin-repos__meta">
                {[
                  t("add.pluginsLine", { number: preview.pluginCount }),
                  t("add.signatureValid"),
                  ...(preview.contact !== null
                    ? [t("add.contactLine", { contact: preview.contact })]
                    : []),
                ].join(" · ")}
              </span>
              {preview.homepage !== null && (
                <span className="plugin-repos__meta">{preview.homepage}</span>
              )}
              <span>
                {t("add.signingKey")}{" "}
                <span className="plugin-repos__fingerprint">{preview.fingerprint}</span>
              </span>
            </div>
          </div>
          {preview.notes.length > 0 && (
            <ul className="plugin-repos__notes">
              {preview.notes.map((note) => (
                <li key={note}>{note}</li>
              ))}
            </ul>
          )}
          <div className="plugin-repos__warning" role="note">
            <strong>
              <span aria-hidden="true">⚠ </span>
              {t("add.warningTitle")}
            </strong>
            <p>{t("add.warningBody")}</p>
          </div>
          <div className="settings-dialog__actions">
            <button
              type="button"
              className="settings-button settings-button--primary"
              ref={cancelRef}
              onClick={onCancel}
            >
              {t("common:cancel")}
            </button>
            <button
              type="button"
              className="settings-button settings-button--danger"
              disabled={busy}
              onClick={() => void add()}
            >
              {t("add.confirm")}
            </button>
          </div>
        </>
      )}
    </ModalDialog>
  );
}
