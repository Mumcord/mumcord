/**
 * "Relay changed on ‹server›" (#600): the old and the new relay fingerprint side by side, when to
 * trust the new relay and when not, and the two choices. Opened from the toast, the notification
 * centre or the chat hint; closes by itself once the relay no longer waits for a decision.
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ModalDialog } from "../settings/components/ModalDialog";
import { useServers } from "../servers";
import { closeRelayDetails, needsDecision, trustNewRelay, useRelayDetails } from "./relayTrust";
import { useRelayInfo } from "./useRelayInfo";
import "./p2p.css";

export function RelayTrustDialog() {
  const serverId = useRelayDetails();
  if (serverId === null) return null;
  return <RelayTrustDetails serverId={serverId} />;
}

function RelayTrustDetails({ serverId }: { serverId: number }) {
  const { t } = useTranslation("p2p");
  const info = useRelayInfo(serverId);
  const { servers } = useServers();
  const server = servers.find((entry) => entry.id === serverId);
  const name = server ? server.label || server.address : "";
  const [failed, setFailed] = useState(false);
  const waiting = info !== null && needsDecision(info);

  useEffect(() => {
    // Trusted or kept blocked elsewhere: nothing is left to decide here.
    if (info !== null && !waiting) closeRelayDetails();
  }, [info, waiting]);

  if (!waiting) return null;
  return (
    <ModalDialog
      title={t("trust.title", { server: name })}
      onClose={closeRelayDetails}
      className="relay-trust"
    >
      <p>{t("trust.dialogBody")}</p>
      <dl className="relay-trust__fingerprints">
        <dt>{t("trust.previous")}</dt>
        <dd>
          <code>{info.previousFingerprint ?? t("trust.unknown")}</code>
        </dd>
        <dt>{t("trust.current")}</dt>
        <dd>
          <code>{info.fingerprint ?? t("trust.unknown")}</code>
        </dd>
      </dl>
      <div className="relay-trust__advice">
        <section>
          <h3 className="relay-trust__yes">{t("trust.trustIf")}</h3>
          <p>{t("trust.trustIfText")}</p>
        </section>
        <section>
          <h3 className="relay-trust__no">{t("trust.dontTrustIf")}</h3>
          <p>{t("trust.dontTrustIfText")}</p>
        </section>
      </div>
      {failed && (
        <p role="alert" className="settings-error">
          {t("relay.actionFailed")}
        </p>
      )}
      <div className="settings-dialog__actions">
        <button type="button" className="settings-button" onClick={closeRelayDetails}>
          {t("trust.notNow")}
        </button>
        <button
          type="button"
          className="settings-button settings-button--primary"
          onClick={() => {
            setFailed(false);
            trustNewRelay(serverId).then(closeRelayDetails, (error: unknown) => {
              console.error("Trusting the new relay failed", error);
              setFailed(true);
            });
          }}
        >
          {t("relay.trustNew")}
        </button>
      </div>
    </ModalDialog>
  );
}
