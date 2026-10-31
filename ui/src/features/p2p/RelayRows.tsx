/**
 * The "Mumbot" and "Mumpeer relay" lines of the server info panel (FR-4.9, FR-4.10, FR-4.20)
 * with the trust prompt after a relay change and "Forget relay" for a pinned relay. Reading
 * these values sends nothing to the server.
 */
import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type RelayInfo } from "../../api";
import { relayReasonText } from "./relayReason";
import { useRelayInfo } from "./useRelayInfo";
import "./p2p.css";

export type RelayAction = "trustNew" | "keepBlocked" | "forget";

type RowComponent = (props: { label: string; children: ReactNode }) => ReactNode;

/** Live rows for `serverId`; nothing until the first read answered. */
export function RelayRows({ serverId, Row }: { serverId: number | null; Row: RowComponent }) {
  const info = useRelayInfo(serverId);
  const [failed, setFailed] = useState(false);
  if (serverId === null || info === null) return null;

  const run = (action: RelayAction) => {
    setFailed(false);
    const api = getApi();
    const call =
      action === "trustNew"
        ? api.relayTrustNew(serverId)
        : action === "keepBlocked"
          ? api.relayKeepBlocked(serverId)
          : api.relayForget(serverId);
    call.catch((error: unknown) => {
      console.error(`Relay action ${action} failed`, error);
      setFailed(true);
    });
  };
  return <RelayRowsView info={info} failed={failed} onAction={run} Row={Row} />;
}

export function RelayRowsView({
  info,
  failed,
  onAction,
  Row,
}: {
  info: RelayInfo;
  failed: boolean;
  onAction: (action: RelayAction) => void;
  Row: RowComponent;
}) {
  const { t } = useTranslation("p2p");
  const { status } = info;
  const bots =
    info.bots.length > 0 ? t("relay.yesNamed", { names: info.bots.join(", ") }) : t("relay.no");
  // A pin exists once a relay was accepted; a changed relay offers its own choices instead.
  const canForget = info.firstSeen !== null && status.kind !== "untrusted";

  let state: ReactNode;
  switch (status.kind) {
    case "none":
      state = t("relay.no");
      break;
    case "available":
      state = info.host ? t("relay.availableAt", { host: info.host }) : t("relay.available");
      break;
    case "unavailable":
      state = t("relay.unavailable", { reason: relayReasonText(t, status.reason) });
      break;
    case "untrusted":
      state = (
        <>
          {t("relay.untrusted")}
          <span className="relay-rows__note">{t("relay.changed")}</span>
        </>
      );
      break;
    case "conflict":
      state = t("reason.conflict");
      break;
  }

  const button = (action: RelayAction) => (
    <button
      type="button"
      className="button"
      onClick={() => {
        onAction(action);
      }}
    >
      {t(`relay.${action}`)}
    </button>
  );

  return (
    <>
      <Row label={t("relay.mumbot")}>{bots}</Row>
      <Row label={t("relay.relay")}>
        {state}
        {(status.kind === "untrusted" || canForget) && (
          <span className="relay-rows__actions">
            {status.kind === "untrusted" && button("trustNew")}
            {status.kind === "untrusted" && !status.keptBlocked && button("keepBlocked")}
            {canForget && button("forget")}
          </span>
        )}
        {failed && (
          <span className="relay-rows__error" role="alert">
            {t("relay.actionFailed")}
          </span>
        )}
      </Row>
    </>
  );
}
