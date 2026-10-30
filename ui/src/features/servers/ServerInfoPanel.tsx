import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { ConnectionInfo } from "../connection";
import type { ServerState } from "../channels/serverStore";
import { Dialog } from "./Dialog";
import "./server-info-panel.css";

interface ServerInfoPanelProps {
  serverState: ServerState;
  onClose: () => void;
}

export function ServerInfoPanel({ serverState, onClose }: ServerInfoPanelProps) {
  const { t } = useTranslation("servers");
  const { info } = serverState;
  const [copied, setCopied] = useState<"address" | "diagnostics" | "fingerprint" | null>(null);
  const [showCertificate, setShowCertificate] = useState(false);

  if (!info) return null;

  const rows: [string, string | null][] = [
    [t("serverInfo.name"), info.serverLabel],
    [
      t("serverInfo.address"),
      info.serverAddress ? `${info.serverAddress}:${info.serverPort?.toString() ?? ""}` : null,
    ],
    [t("serverInfo.version"), info.version || null],
    [t("serverInfo.release"), info.release],
    [t("serverInfo.os"), info.os],
    [t("serverInfo.maxUsers"), info.maxUsers?.toString() ?? null],
    [t("serverInfo.maxBandwidth"), info.maxBandwidth?.toString() ?? null],
    [t("serverInfo.tlsFingerprint"), info.tlsFingerprint],
    [t("serverInfo.tlsIssuer"), info.tlsIssuer],
    [t("serverInfo.tlsValidFrom"), info.tlsValidFrom],
    [t("serverInfo.tlsValidUntil"), info.tlsValidUntil],
    [t("serverInfo.codecs"), "Opus"],
    [t("serverInfo.usersOnline"), info.usersOnline.toString()],
    [t("serverInfo.sessionId"), info.sessionId?.toString() ?? null],
    [t("serverInfo.registered"), info.registered ? t("serverInfo.yes") : t("serverInfo.no")],
    [t("serverInfo.permissions"), info.currentChannelPermissions?.toString() ?? null],
    [t("serverInfo.mumcordUsers"), info.mumcordUsers.toString()],
    [t("serverInfo.tlsTrusted"), info.tlsTrusted ? t("serverInfo.yes") : t("serverInfo.no")],
  ];

  const copy = async (text: string, kind: "address" | "diagnostics" | "fingerprint") => {
    try {
      await getApi().copyText(text);
      setCopied(kind);
      setTimeout(() => {
        setCopied(null);
      }, 2000);
    } catch {
      // The copy API logs the real reason; the user only needs to know it did not work.
    }
  };

  const copyDiagnostics = () => {
    const lines = rows
      .filter(([, value]) => value !== null)
      .map(([label, value]) => `${label}: ${value ?? ""}`);
    if (info.welcomeText) lines.push(`${t("serverInfo.welcomeText")}: ${info.welcomeText}`);
    void copy(lines.join("\n"), "diagnostics");
  };

  return (
    <Dialog title={t("serverInfo.title")} onClose={onClose}>
      <dl className="server-info-panel__list">
        {rows
          .filter(([, value]) => value)
          .map(([label, value]) => (
            <div key={label} className="server-info-panel__row">
              <dt>{label}</dt>
              <dd>{value}</dd>
            </div>
          ))}
      </dl>
      {info.welcomeText && (
        <div className="server-info-panel__welcome">
          <h3>{t("serverInfo.welcomeText")}</h3>
          <div dangerouslySetInnerHTML={{ __html: info.welcomeText }} />
        </div>
      )}
      <div className="server-info-panel__connection">
        <h3>{t("serverInfo.connection")}</h3>
        <ConnectionInfo />
      </div>
      {info.tlsCertificatePem && (
        <div className="server-info-panel__certificate">
          <button
            type="button"
            className="button"
            aria-expanded={showCertificate}
            onClick={() => {
              setShowCertificate((open) => !open);
            }}
          >
            {t("serverInfo.showCertificate")}
          </button>
          {showCertificate && (
            <pre className="server-info-panel__pem">{info.tlsCertificatePem}</pre>
          )}
        </div>
      )}
      <div className="server-info-panel__actions">
        {info.serverAddress && (
          <button
            type="button"
            className="button"
            onClick={() => {
              void copy(
                `${info.serverAddress ?? ""}:${info.serverPort?.toString() ?? ""}`,
                "address",
              );
            }}
          >
            {copied === "address" ? t("serverInfo.copied") : t("serverInfo.copyAddress")}
          </button>
        )}
        {info.tlsFingerprint && (
          <button
            type="button"
            className="button"
            onClick={() => {
              void copy(info.tlsFingerprint ?? "", "fingerprint");
            }}
          >
            {copied === "fingerprint" ? t("serverInfo.copied") : t("serverInfo.copyFingerprint")}
          </button>
        )}
        <button type="button" className="button" onClick={copyDiagnostics}>
          {copied === "diagnostics" ? t("serverInfo.copied") : t("serverInfo.copyDiagnostics")}
        </button>
        <button type="button" className="button button--primary" onClick={onClose}>
          {t("serverInfo.close")}
        </button>
      </div>
    </Dialog>
  );
}
