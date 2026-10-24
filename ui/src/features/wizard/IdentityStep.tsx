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
  focusOnMount: boolean;
  onDone: (identity: Identity) => void;
}

/** Step 1: create a new identity or import a PKCS#12 file (FR-1.2). */
export function IdentityStep({ identity, focusOnMount, onDone }: IdentityStepProps) {
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
          ? await getApi().identityCreate(name.trim())
          : await getApi().identityImport(path ?? "", password === "" ? null : password);
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
      step={1}
      title={t("identity.title")}
      focusOnMount={focusOnMount}
      busy={busy}
      submitLabel={t("common:next")}
      onSubmit={() => void submit()}
    >
      {identity ? (
        <p>{t("identity.existing", { name: identity.name })}</p>
      ) : (
        <>
          <p className="wizard-muted">{t("identity.intro")}</p>
          <fieldset className="wizard-choice">
            <legend className="visually-hidden">{t("identity.title")}</legend>
            <label className="wizard-radio">
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
              {t("identity.create")}
            </label>
            {mode === "create" && (
              <div className="wizard-field wizard-indent">
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
            <label className="wizard-radio">
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
              {t("identity.import")}
            </label>
            {mode === "import" && (
              <div className="wizard-indent">
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
                </div>
              </div>
            )}
          </fieldset>
        </>
      )}
      {error && (
        <p id={errorId} role="alert" className="wizard-error">
          {error}
        </p>
      )}
      <p className="wizard-note">{t("identity.backup")}</p>
    </StepFrame>
  );
}

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
