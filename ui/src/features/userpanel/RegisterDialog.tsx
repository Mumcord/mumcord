import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi } from "../../api";
import { describeError } from "./errors";
import { pickSavePath } from "./files";
import { Modal } from "./Modal";

interface RegisterDialogProps {
  onClose: () => void;
}

export function RegisterDialog({ onClose }: RegisterDialogProps) {
  const { t } = useTranslation(["userpanel", "common"]);
  const passwordId = useId();
  const errorId = useId();
  const [password, setPassword] = useState("");
  const [exported, setExported] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const exportIdentity = async () => {
    if (password === "") {
      setError(t("register.passwordRequired"));
      return;
    }
    setError(null);
    try {
      const path = await pickSavePath(t("register.saveTitle"), "mumcord-identity.p12");
      if (path === null) return;
      await getApi().identityExport(path, password);
      setExported(true);
      announce(t("register.exported"));
    } catch (e) {
      setError(describeError(t, e));
    }
  };

  const register = async () => {
    setBusy(true);
    setError(null);
    try {
      await getApi().registerSelf();
      announce(t("register.done"));
      onClose();
    } catch (e) {
      setError(describeError(t, e));
      setBusy(false);
    }
  };

  return (
    <Modal title={t("register.title")} onClose={onClose}>
      <div className="userpanel-form">
        <p>{t("register.explain")}</p>
        <p>{t("register.backup")}</p>
        <label htmlFor={passwordId}>{t("register.password")}</label>
        <input
          id={passwordId}
          type="password"
          autoComplete="new-password"
          value={password}
          aria-describedby={error ? errorId : undefined}
          onChange={(e) => {
            setPassword(e.target.value);
          }}
        />
        <button type="button" className="button" onClick={() => void exportIdentity()}>
          {t("register.export")}
        </button>
        {exported ? (
          <p className="userpanel-hint" role="status">
            {t("register.exported")}
          </p>
        ) : null}
        {error ? (
          <p id={errorId} className="userpanel-error" role="alert">
            {error}
          </p>
        ) : null}
        <div className="userpanel-form__actions">
          <button type="button" className="button" onClick={onClose}>
            {t("common:cancel")}
          </button>
          <button
            type="button"
            className="button button--primary"
            disabled={busy}
            onClick={() => void register()}
          >
            {t("register.confirm")}
          </button>
        </div>
      </div>
    </Modal>
  );
}
