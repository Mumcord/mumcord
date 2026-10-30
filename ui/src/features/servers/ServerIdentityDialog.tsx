import { useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { SavedServer } from "../../api";
import { openSettingsSection } from "../settings/settingsOpener";
import { Dialog } from "./Dialog";
import { displayName } from "./format";
import { IdentitySelect } from "./IdentitySelect";
import { setServerIdentity, useServers } from "./store";

interface ServerIdentityDialogProps {
  server: SavedServer;
  onClose: () => void;
}

/**
 * "Identity for this server" (issue #148): picks the certificate this server connects with.
 * Saving while connected asks whether to reconnect now; only this server is reconnected.
 */
export function ServerIdentityDialog({ server, onClose }: ServerIdentityDialogProps) {
  const { t } = useTranslation(["servers", "common"]);
  const identities = useServers().identities;
  const connected = useServers().connections.has(server.id);
  const [choice, setChoice] = useState<number | null>(server.identityId);
  const [asking, setAsking] = useState(false);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const hintId = useId();
  const changed = choice !== server.identityId;

  const save = async (reconnect: boolean) => {
    const saved = await setServerIdentity(server.id, choice, reconnect);
    if (saved) onClose();
  };

  const label = displayName(server);
  const chosen =
    identities.find((identity) => identity.id === choice)?.label ??
    identities.find((identity) => identity.isDefault)?.label ??
    "";

  if (asking) {
    return (
      <Dialog
        title={t("identity.reconnectTitle", { server: label })}
        onClose={onClose}
        initialFocus={cancelRef}
      >
        <p>{t("identity.reconnectMessage", { identity: chosen })}</p>
        <div className="servers-dialog__actions">
          <button ref={cancelRef} type="button" className="button" onClick={() => void save(false)}>
            {t("identity.later")}
          </button>
          <button type="button" className="button button--primary" onClick={() => void save(true)}>
            {t("identity.reconnect")}
          </button>
        </div>
      </Dialog>
    );
  }

  return (
    <Dialog title={t("identity.title", { server: label })} onClose={onClose}>
      <div className="servers-form">
        <p className="servers-form__hint">{t("identity.intro")}</p>
        <IdentitySelect
          label={t("identity.field")}
          value={choice}
          identities={identities}
          onChange={setChoice}
          hint={t("identity.hint")}
          hintId={hintId}
        />
        <p className="servers-form__hint">{t("identity.manageHint")}</p>
        <button
          type="button"
          className="button servers-form__link"
          onClick={() => {
            onClose();
            openSettingsSection("identity");
          }}
        >
          {t("identity.manage")}
        </button>
        <div className="servers-dialog__actions">
          <button ref={cancelRef} type="button" className="button" onClick={onClose}>
            {t("common:cancel")}
          </button>
          <button
            type="button"
            className="button button--primary"
            disabled={!changed}
            onClick={() => {
              if (connected) setAsking(true);
              else void save(false);
            }}
          >
            {connected ? t("identity.saveAndReconnect") : t("identity.save")}
          </button>
        </div>
      </div>
    </Dialog>
  );
}
