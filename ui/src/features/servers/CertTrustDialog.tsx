import { useId, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { CertTrustRequired } from "../../api";
import { Dialog } from "./Dialog";

interface CertTrustDialogProps {
  prompt: CertTrustRequired;
  serverName: string;
  onDecide: (trust: boolean) => void;
}

/**
 * Asks whether to trust a server certificate (FR-1.10). A changed certificate is a blocking
 * warning: focus starts on "Don't connect" and trusting is styled as the risky choice.
 */
export function CertTrustDialog({ prompt, serverName, onDecide }: CertTrustDialogProps) {
  const { t } = useTranslation("servers");
  const descriptionId = useId();
  const rejectRef = useRef<HTMLButtonElement>(null);
  const changed = prompt.previousFingerprint !== null;
  const reject = () => {
    onDecide(false);
  };

  return (
    <Dialog
      title={changed ? t("cert.changedTitle") : t("cert.newTitle")}
      role={changed ? "alertdialog" : "dialog"}
      describedBy={descriptionId}
      initialFocus={changed ? rejectRef : undefined}
      onClose={reject}
    >
      <div id={descriptionId}>
        {changed ? (
          <>
            <p className="servers-cert__warning">
              {t("cert.changedWarning", { server: serverName })}
            </p>
            <p>{t("cert.changedAdvice")}</p>
          </>
        ) : (
          <p>{t("cert.newIntro", { server: serverName })}</p>
        )}
      </div>
      <dl className="servers-cert__fingerprints">
        {prompt.previousFingerprint !== null && (
          <>
            <dt>{t("cert.previousFingerprint")}</dt>
            <dd>
              <code>{prompt.previousFingerprint}</code>
            </dd>
          </>
        )}
        <dt>{changed ? t("cert.newFingerprint") : t("cert.fingerprint")}</dt>
        <dd>
          <code>{prompt.fingerprint}</code>
        </dd>
      </dl>
      <div className="servers-dialog__actions">
        <button
          ref={rejectRef}
          type="button"
          className={changed ? "button button--primary" : "button"}
          onClick={reject}
        >
          {t("cert.reject")}
        </button>
        <button
          type="button"
          className={changed ? "button servers-button--danger" : "button button--primary"}
          onClick={() => {
            onDecide(true);
          }}
        >
          {changed ? t("cert.trustChanged") : t("cert.trust")}
        </button>
      </div>
    </Dialog>
  );
}
