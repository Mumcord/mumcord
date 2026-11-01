import { useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type Identity } from "../../api";
import { errorCode } from "./errors";
import { pickIdentityFile } from "./pickIdentityFile";
import { StepFrame } from "./StepFrame";

type Mode = "create" | "import";

interface IdentityStepProps {
  /** Identity that already exists (wizard was left halfway); the step then only confirms it. */
  identity: Identity | null;
  onDone: (identity: Identity) => void;
}

/**
 * "Who are you on Mumble?" (first start only): create a new identity or import a PKCS#12 file
 * (FR-1.2). The identity is created on Next, since the later steps need it.
 */
export function IdentityStep({ identity, onDone }: IdentityStepProps) {
  const { t } = useTranslation("wizard");
  const [mode, setMode] = useState<Mode>("create");
  const [name, setName] = useState("");
  const [path, setPath] = useState<string | null>(null);
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const nameInput = useRef<HTMLInputElement>(null);
  const chooseButton = useRef<HTMLButtonElement>(null);
  const errorId = useId();
  const nameHintId = useId();
  const fileStatusId = useId();

  function failWith(message: string, focus: HTMLElement | null) {
    setError(message);
    focus?.focus();
  }

  async function choose() {
    try {
      const chosen = await pickIdentityFile(t("identity.fileType"));
      if (chosen !== null) {
        setPath(chosen);
        setError(null);
      }
    } catch (cause: unknown) {
      console.error("identity file picker failed", cause);
      setError(t("identity.pickFailed"));
    }
  }

  async function submit() {
    if (identity) {
      onDone(identity);
      return;
    }
    if (mode === "create" && name.trim() === "") {
      failWith(t("identity.nameRequired"), nameInput.current);
      return;
    }
    if (mode === "import" && path === null) {
      failWith(t("identity.fileRequired"), chooseButton.current);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const created =
        mode === "create"
          ? await getApi().identityCreate(name.trim(), null)
          : await getApi().identityImport(path ?? "", password === "" ? null : password, null);
      onDone(created);
    } catch (cause: unknown) {
      setBusy(false);
      failWith(messageFor(cause), mode === "create" ? nameInput.current : chooseButton.current);
    }
  }

  function messageFor(cause: unknown): string {
    const code = errorCode(cause);
    if (code === "secureStorageUnavailable") return t("errors.secureStorageUnavailable");
    if (mode === "create") {
      return code === "invalidInput" ? t("identity.nameRequired") : t("errors.generic");
    }
    switch (code) {
      case "invalidInput":
        return t("identity.importInvalid");
      case "notFound":
        return t("identity.importNotFound");
      case "io":
        return t("identity.importIo");
      default:
        return t("errors.generic");
    }
  }

  const describedBy = (...ids: string[]) => [...ids, error ? errorId : ""].join(" ").trim();

  return (
    <StepFrame
      title={t("identity.title")}
      subtitle={t("identity.intro")}
      busy={busy}
      onSubmit={() => void submit()}
    >
      {identity ? (
        <p className="wizard-card">{t("identity.existing", { name: identity.name })}</p>
      ) : (
        <fieldset className="wizard-columns">
          <legend className="visually-hidden">{t("identity.legend")}</legend>
          <div className="wizard-option wizard-identity-card" data-selected={mode === "create"}>
            <label className="wizard-option__label">
              <input
                type="radio"
                name="identity-mode"
                value="create"
                checked={mode === "create"}
                onChange={() => {
                  setMode("create");
                  setError(null);
                }}
              />
              <span className="wizard-option__title">{t("identity.create")}</span>
            </label>
            <p className="wizard-muted wizard-small">{t("identity.createText")}</p>
            {mode === "create" && (
              <div className="wizard-field">
                <label htmlFor={`${errorId}-name`}>{t("identity.name")}</label>
                <input
                  id={`${errorId}-name`}
                  ref={nameInput}
                  type="text"
                  autoComplete="nickname"
                  value={name}
                  aria-invalid={error !== null}
                  aria-describedby={describedBy(nameHintId)}
                  onChange={(event) => {
                    setName(event.target.value);
                  }}
                />
                <p id={nameHintId} className="wizard-muted wizard-small">
                  {t("identity.nameHint")}
                </p>
              </div>
            )}
            <p className="wizard-note wizard-small">{t("identity.backup")}</p>
          </div>
          <div className="wizard-option wizard-identity-card" data-selected={mode === "import"}>
            <label className="wizard-option__label">
              <input
                type="radio"
                name="identity-mode"
                value="import"
                checked={mode === "import"}
                onChange={() => {
                  setMode("import");
                  setError(null);
                }}
              />
              <span className="wizard-option__title">{t("identity.import")}</span>
            </label>
            {mode === "import" && (
              <>
                <div className="wizard-row">
                  <button
                    type="button"
                    ref={chooseButton}
                    className="button"
                    aria-describedby={describedBy(fileStatusId)}
                    onClick={() => void choose()}
                  >
                    {t("identity.chooseFile")}
                  </button>
                  <span id={fileStatusId} className="wizard-muted">
                    {path === null
                      ? t("identity.noFile")
                      : t("identity.chosenFile", { file: fileName(path) })}
                  </span>
                </div>
                <div className="wizard-field">
                  <label htmlFor={`${errorId}-password`}>{t("identity.password")}</label>
                  <input
                    id={`${errorId}-password`}
                    type="password"
                    autoComplete="off"
                    value={password}
                    onChange={(event) => {
                      setPassword(event.target.value);
                    }}
                  />
                  <p className="wizard-muted wizard-small">{t("identity.passwordHint")}</p>
                </div>
              </>
            )}
          </div>
        </fieldset>
      )}
      {error && (
        <p id={errorId} role="alert" className="wizard-error">
          {error}
        </p>
      )}
    </StepFrame>
  );
}

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
