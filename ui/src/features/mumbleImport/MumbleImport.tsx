import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import {
  getApi,
  toEngineError,
  type MumbleImportDone,
  type MumbleImportFound,
  type Settings,
} from "../../api";
import { ImportChooser, SelectedCount } from "./ImportChooser";
import { ImportSummary, type ImportOutcome } from "./ImportSummary";
import {
  importRequest,
  initialSelection,
  previousValues,
  selectedCount,
  settingsPatch,
  type ImportSelection,
} from "./selection";
import "./mumbleImport.css";

export type ImportStage = "choose" | "summary";

interface MumbleImportProps {
  found: MumbleImportFound;
  settings: Settings;
  /** Saves changed settings through the normal settings update. */
  saveSettings: (patch: Partial<Settings>) => Promise<void>;
  /** Heading and intro for each stage; the wizard and Settings frame it differently. */
  header: (stage: ImportStage) => ReactNode;
  /** "Skip for now" in the wizard, "Cancel" in Settings. */
  dismissLabel: string;
  onDismiss: () => void;
  /** "Done" on the summary. */
  onFinish: () => void;
  /** After an import or its undo, so lists elsewhere can be read again. */
  onChanged?: (done: MumbleImportDone | null) => void;
}

/**
 * Import from the official Mumble client (FR-3.93, FR-3.94): a preview with a checkbox per
 * part, then a summary with undo. Passwords and the certificate stay in the engine.
 */
export function MumbleImport({
  found,
  settings,
  saveSettings,
  header,
  dismissLabel,
  onDismiss,
  onFinish,
  onChanged,
}: MumbleImportProps) {
  const { t } = useTranslation("mumbleImport");
  const [selection, setSelection] = useState<ImportSelection>(() => initialSelection(found));
  const [password, setPassword] = useState("");
  const [passwordError, setPasswordError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<ImportOutcome | null>(null);
  const passwordRef = useRef<HTMLInputElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const stageChanged = useRef(false);
  const count = selectedCount(found, selection);

  async function runImport() {
    if (busy || count === 0) return;
    const request = importRequest(found, selection, password);
    if (request.certificatePassword === "") {
      setPasswordError(t("identity.passwordRequired"));
      passwordRef.current?.focus();
      return;
    }
    setBusy(true);
    setError(null);
    setPasswordError(null);
    try {
      const done = await getApi().mumbleImportApply(request);
      const patch = settingsPatch(found, selection);
      const before = previousValues(settings, patch);
      let settingsFailed = false;
      if (Object.keys(patch).length > 0) {
        try {
          await saveSettings(patch);
        } catch (cause: unknown) {
          console.error("saving the imported settings failed", toEngineError(cause).code);
          settingsFailed = true;
        }
      }
      const deviceNames = [found.inputDevice?.name, found.outputDevice?.name].filter(
        (name): name is string => name !== undefined,
      );
      // Servers already in Mumcord were left out of the request; the summary still lists them.
      const duplicates = selection.items.has("favourites")
        ? found.favourites.filter((f) => f.duplicate).map((f) => f.label)
        : [];
      const skipped = [...new Set([...duplicates, ...done.skipped])];
      setOutcome({ done: { ...done, skipped }, patch, before, deviceNames, settingsFailed });
      onChanged?.(done);
      announce(t("summary.title"), "polite");
    } catch (cause: unknown) {
      const code = toEngineError(cause).code;
      if (code === "wrongPassword") {
        setPasswordError(t("identity.wrongPassword"));
        passwordRef.current?.focus();
      } else {
        console.error("the Mumble import failed", code);
        setError(code === "secureStorageUnavailable" ? t("secureStorage") : t("failed"));
      }
    } finally {
      setBusy(false);
    }
  }

  async function undo() {
    if (busy || !outcome) return;
    setBusy(true);
    setError(null);
    try {
      await getApi().mumbleImportUndo();
      if (Object.keys(outcome.before).length > 0) await saveSettings(outcome.before);
      setOutcome(null);
      onChanged?.(null);
      announce(t("summary.undone"), "polite");
    } catch (cause: unknown) {
      console.error("undoing the Mumble import failed", toEngineError(cause).code);
      setError(t("summary.undoFailed"));
    } finally {
      setBusy(false);
    }
  }

  // After the import and after its undo, focus the new stage's heading (it says what happened).
  useEffect(() => {
    if (!stageChanged.current) {
      stageChanged.current = true;
      return;
    }
    root.current?.querySelector<HTMLElement>("[data-stage-heading]")?.focus();
  }, [outcome]);

  const errorLine = error !== null && (
    <p className="mumble-import__error" role="alert">
      {error}
    </p>
  );

  if (outcome) {
    return (
      <div className="mumble-import" ref={root}>
        {header("summary")}
        <ImportSummary outcome={outcome} />
        {errorLine}
        <div className="mumble-import__footer">
          <button type="button" className="button" aria-disabled={busy} onClick={() => void undo()}>
            {t("summary.undo")}
          </button>
          <button type="button" className="button button--primary" onClick={onFinish}>
            {t("summary.done")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="mumble-import" ref={root}>
      {header("choose")}
      <ImportChooser
        found={found}
        selection={selection}
        onSelectionChange={setSelection}
        certificatePassword={password}
        onCertificatePasswordChange={(value) => {
          setPassword(value);
          setPasswordError(null);
        }}
        passwordRef={passwordRef}
        passwordError={passwordError}
      />
      {errorLine}
      <div className="mumble-import__footer">
        <button type="button" className="button" onClick={onDismiss}>
          {dismissLabel}
        </button>
        <div className="mumble-import__submit">
          <SelectedCount found={found} selection={selection} />
          <button
            type="button"
            className="button button--primary"
            aria-disabled={busy || count === 0}
            aria-busy={busy}
            onClick={() => void runImport()}
          >
            {busy ? t("importing") : t("import")}
          </button>
        </div>
      </div>
    </div>
  );
}
