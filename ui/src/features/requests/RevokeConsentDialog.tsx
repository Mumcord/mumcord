import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { toEngineError } from "../../api";
import { ConfirmDialog } from "../servers";
import { closeRevokeConsentDialog, useRevokeConsentTarget } from "./revokeConsentStore";
import { revokeAccepted } from "./requestsStore";

/** Renders the "Revoke chat consent" confirmation while one is open; mount once. */
export function RevokeConsentDialogHost() {
  const { t } = useTranslation("members");
  const target = useRevokeConsentTarget();
  if (!target) return null;
  const { conversation, opener } = target;
  const close = () => {
    closeRevokeConsentDialog();
    if (opener?.isConnected) opener.focus();
  };
  return (
    <ConfirmDialog
      title={t("revokeConsent.title", { name: conversation.name })}
      message={t("revokeConsent.message", { name: conversation.name })}
      confirmLabel={t("revokeConsent.confirm")}
      danger
      onConfirm={() => {
        close();
        revokeAccepted(conversation).catch((error: unknown) => {
          console.error("Revoking chat consent failed", toEngineError(error).code);
          announce(t("revokeConsent.failed"), "assertive");
        });
      }}
      onClose={close}
    />
  );
}
