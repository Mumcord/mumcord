import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import type { ServerState } from "../channels/serverStore";
import { useConnectionStats, type StatsState } from "../connection/useConnectionStats";
import { RelayRows } from "../p2p/RelayRows";
import { Dialog } from "./Dialog";
import { initials } from "./format";
import "./server-info-panel.css";

/** Mumble ACL bits, in the order the panel lists them. */
const PERMISSION_KEYS: { bit: number; key: string }[] = [
  { bit: 0x8, key: "permissionSpeak" },
  { bit: 0x1, key: "permissionWrite" },
  { bit: 0x100, key: "permissionWhisper" },
  { bit: 0x200, key: "permissionTextMessage" },
  { bit: 0x800, key: "permissionListen" },
  { bit: 0x4, key: "permissionEnter" },
  { bit: 0x40, key: "permissionMakeChannel" },
  { bit: 0x80, key: "permissionLinkChannel" },
  { bit: 0x20, key: "permissionMove" },
  { bit: 0x400, key: "permissionMakeTempChannel" },
  { bit: 0x10, key: "permissionMuteDeafen" },
  { bit: 0x1_0000, key: "permissionKick" },
  { bit: 0x2_0000, key: "permissionBan" },
  { bit: 0x4_0000, key: "permissionRegister" },
  { bit: 0x8_0000, key: "permissionSelfRegister" },
  { bit: 0x10_0000, key: "permissionResetUserContent" },
];

interface ServerInfoPanelProps {
  serverState: ServerState;
  onClose: () => void;
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="server-info-panel__row">
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function ConnectionCard({
  state,
  address,
  onCopyAddress,
  copied,
}: {
  state: StatsState;
  address: string;
  onCopyAddress: () => void;
  copied: boolean;
}) {
  const { t } = useTranslation("servers");
  const { t: tc } = useTranslation("connection");
  return (
    <section className="server-info-panel__card">
      <h3>{t("serverInfo.connection")}</h3>
      {state.status === "ready" ? (
        <dl className="server-info-panel__list">
          <Row label={t("serverInfo.transport")}>
            {state.stats.transport ? tc(`transportValue.${state.stats.transport}`) : tc("none")}
          </Row>
          <Row label={t("serverInfo.ping")}>
            {state.stats.rttMs === null
              ? tc("none")
              : tc("latencyValue", { value: Math.round(state.stats.rttMs) })}
          </Row>
          <Row label={t("serverInfo.packetLoss")}>
            {tc("lossValue", { value: state.stats.lossPercent })}
          </Row>
          <Row label={t("serverInfo.bandwidthInUse")}>
            {state.stats.serverMaxBandwidthKbps === null
              ? tc("bitrateValue", { value: state.stats.sendBitrateKbps })
              : t("serverInfo.bandwidthOf", {
                  used: state.stats.sendBitrateKbps,
                  limit: state.stats.serverMaxBandwidthKbps,
                })}
          </Row>
          <Row label={t("serverInfo.address")}>{address || t("serverInfo.none")}</Row>
        </dl>
      ) : (
        <p className="server-info-panel__note">
          {state.status === "loading"
            ? tc("loading")
            : state.status === "error"
              ? tc("error")
              : tc("notConnected")}
        </p>
      )}
      {address && (
        <div className="server-info-panel__card-actions">
          <button type="button" className="button" onClick={onCopyAddress}>
            {copied ? t("serverInfo.copied") : t("serverInfo.copyAddress")}
          </button>
        </div>
      )}
    </section>
  );
}

/**
 * Everything the client can learn about a server without a server change (FR-1.10): server and
 * session facts, the TLS certificate with its issuer and validity, live connection figures and
 * the welcome text. Copy buttons put the values on the clipboard for bug reports.
 */
export function ServerInfoPanel({ serverState, onClose }: ServerInfoPanelProps) {
  const { t } = useTranslation("servers");
  const { t: tc } = useTranslation("connection");
  const { info } = serverState;
  const [copied, setCopied] = useState<"address" | "diagnostics" | "fingerprint" | null>(null);
  const [showCertificate, setShowCertificate] = useState(false);
  const connection = useConnectionStats(true);

  if (!info) return null;

  const address = info.serverAddress
    ? `${info.serverAddress}:${info.serverPort?.toString() ?? ""}`
    : "";
  const title = info.serverLabel ?? (address || t("serverInfo.title"));
  const versionLine = info.version
    ? info.os
      ? t("serverInfo.versionOnOs", { version: info.version, os: info.os })
      : t("serverInfo.versionOnly", { version: info.version })
    : "";
  const subtitle = [address, versionLine].filter(Boolean).join(" · ");

  const ownUser = info.sessionId === null ? undefined : serverState.users.get(info.sessionId);
  const ownChannel =
    ownUser === undefined ? undefined : serverState.channels.get(ownUser.channelId);
  const channelName = ownChannel?.name ?? "";
  const permissionBits = info.currentChannelPermissions ?? 0;
  const permissionNames = PERMISSION_KEYS.filter(({ bit }) => (permissionBits & bit) !== 0).map(
    ({ key }) => t(`serverInfo.${key}`),
  );
  const permissionText =
    permissionNames.length > 0 ? permissionNames.join(", ") : t("serverInfo.nothing");
  const registeredText = info.registered
    ? t("serverInfo.registeredAs", { name: ownUser?.name ?? "" })
    : t("serverInfo.no");
  const tlsStatus = info.tlsTrusted ? t("serverInfo.trustedUnchanged") : t("serverInfo.notTrusted");
  const maxUsers = info.maxUsers ?? 0;
  const usersPercent =
    maxUsers > 0 ? Math.min(100, Math.round((info.usersOnline / maxUsers) * 100)) : 0;
  const formatLimit = (value: number) =>
    value === 0
      ? t("serverInfo.unlimited")
      : t("serverInfo.characters", { value: value.toLocaleString() });
  const validRange =
    info.tlsValidFrom && info.tlsValidUntil
      ? t("serverInfo.validRange", { from: info.tlsValidFrom, to: info.tlsValidUntil })
      : (info.tlsValidFrom ?? info.tlsValidUntil ?? t("serverInfo.none"));

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
    const lines: string[] = [];
    const push = (label: string, value: string | number | null | undefined) => {
      if (value === null || value === undefined || value === "") return;
      lines.push(`${label}: ${String(value)}`);
    };
    push(t("serverInfo.version"), info.version);
    push(t("serverInfo.os"), info.os);
    push(
      t("serverInfo.usersOnline"),
      maxUsers > 0
        ? t("serverInfo.usersOf", { online: info.usersOnline, max: maxUsers })
        : info.usersOnline,
    );
    push(
      t("serverInfo.maxBandwidth"),
      info.maxBandwidth === null ? null : `${String(Math.round(info.maxBandwidth / 1000))} kbit/s`,
    );
    push(t("serverInfo.maxMessageLength"), formatLimit(info.maxMessageLength));
    push(t("serverInfo.maxImageMessageLength"), formatLimit(info.maxImageMessageLength));
    push(t("serverInfo.codecs"), "Opus");
    push(t("serverInfo.sessionId"), info.sessionId);
    push(t("serverInfo.registered"), registeredText);
    push(t("serverInfo.permissions"), permissionText);
    push(
      t("serverInfo.mumcordUsers"),
      t("serverInfo.mumcordUsersOf", { count: info.mumcordUsers, total: info.usersOnline }),
    );
    push(t("serverInfo.tlsTrusted"), tlsStatus);
    push(t("serverInfo.tlsIssuer"), info.tlsIssuer);
    push(t("serverInfo.tlsValidFrom"), info.tlsValidFrom);
    push(t("serverInfo.tlsValidUntil"), info.tlsValidUntil);
    push(t("serverInfo.tlsFingerprint"), info.tlsFingerprint);
    if (connection.status === "ready") {
      const stats = connection.stats;
      push(
        t("serverInfo.transport"),
        stats.transport ? tc(`transportValue.${stats.transport}`) : tc("none"),
      );
      push(
        t("serverInfo.ping"),
        stats.rttMs === null ? tc("none") : tc("latencyValue", { value: Math.round(stats.rttMs) }),
      );
      push(t("serverInfo.packetLoss"), tc("lossValue", { value: stats.lossPercent }));
      push(
        t("serverInfo.bandwidthInUse"),
        stats.serverMaxBandwidthKbps === null
          ? tc("bitrateValue", { value: stats.sendBitrateKbps })
          : t("serverInfo.bandwidthOf", {
              used: stats.sendBitrateKbps,
              limit: stats.serverMaxBandwidthKbps,
            }),
      );
    }
    push(t("serverInfo.address"), address);
    if (info.welcomeText) {
      lines.push(`${t("serverInfo.welcomeText")}: ${info.welcomeText}`);
    }
    void copy(lines.join("\n"), "diagnostics");
  };

  return (
    <Dialog className="server-info-panel" title={title} onClose={onClose}>
      <header className="server-info-panel__header">
        <span className="server-info-panel__avatar" aria-hidden="true">
          {initials(title)}
        </span>
        <div className="server-info-panel__heading">
          <h2 className="server-info-panel__title">{title}</h2>
          {subtitle && <p className="server-info-panel__subtitle">{subtitle}</p>}
        </div>
        <button
          type="button"
          className="server-info-panel__close"
          aria-label={t("serverInfo.close")}
          onClick={onClose}
        >
          <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false">
            <path
              d="M6 6l12 12M18 6L6 18"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
            />
          </svg>
        </button>
      </header>

      <div className="server-info-panel__grid">
        <section className="server-info-panel__card">
          <h3>{t("serverInfo.serverSection")}</h3>
          <dl className="server-info-panel__list">
            <Row label={t("serverInfo.version")}>{info.version || t("serverInfo.none")}</Row>
            <Row label={t("serverInfo.os")}>{info.os ?? t("serverInfo.none")}</Row>
            <Row label={t("serverInfo.usersOnline")}>
              {maxUsers > 0 ? (
                <>
                  <span>
                    {t("serverInfo.usersOf", { online: info.usersOnline, max: maxUsers })}
                  </span>
                  <span className="server-info-panel__meter" aria-hidden="true">
                    <span style={{ width: `${String(usersPercent)}%` }} />
                  </span>
                </>
              ) : (
                info.usersOnline.toString()
              )}
            </Row>
            {info.maxBandwidth !== null && (
              <Row label={t("serverInfo.maxBandwidth")}>
                {`${String(Math.round(info.maxBandwidth / 1000))} kbit/s`}
              </Row>
            )}
            <Row label={t("serverInfo.maxMessageLength")}>{formatLimit(info.maxMessageLength)}</Row>
            <Row label={t("serverInfo.maxImageMessageLength")}>
              {formatLimit(info.maxImageMessageLength)}
            </Row>
            <Row label={t("serverInfo.codecs")}>Opus</Row>
            <RelayRows serverId={serverState.serverId} Row={Row} />
          </dl>
        </section>

        <section className="server-info-panel__card">
          <h3>{t("serverInfo.youSection")}</h3>
          <dl className="server-info-panel__list">
            <Row label={t("serverInfo.sessionId")}>
              {info.sessionId?.toString() ?? t("serverInfo.none")}
            </Row>
            <Row label={t("serverInfo.registered")}>{registeredText}</Row>
            <Row
              label={
                channelName
                  ? t("serverInfo.inChannelYouMay", { channel: channelName })
                  : t("serverInfo.youMay")
              }
            >
              {permissionText}
            </Row>
            <Row label={t("serverInfo.mumcordUsers")}>
              {t("serverInfo.mumcordUsersOf", {
                count: info.mumcordUsers,
                total: info.usersOnline,
              })}
            </Row>
          </dl>
        </section>

        <section className="server-info-panel__card">
          <h3>{t("serverInfo.certificate")}</h3>
          <dl className="server-info-panel__list">
            <Row label={t("serverInfo.tlsStatus")}>
              <span className={info.tlsTrusted ? "server-info-panel__trusted" : undefined}>
                {tlsStatus}
              </span>
            </Row>
            <Row label={t("serverInfo.tlsIssuer")}>{info.tlsIssuer ?? t("serverInfo.none")}</Row>
            <Row label={t("serverInfo.tlsValid")}>{validRange}</Row>
            <Row label={t("serverInfo.sha256")}>
              <span className="server-info-panel__mono">
                {info.tlsFingerprint ?? t("serverInfo.none")}
              </span>
            </Row>
          </dl>
          <div className="server-info-panel__card-actions">
            {info.tlsFingerprint && (
              <button
                type="button"
                className="button"
                onClick={() => {
                  void copy(info.tlsFingerprint ?? "", "fingerprint");
                }}
              >
                {copied === "fingerprint"
                  ? t("serverInfo.copied")
                  : t("serverInfo.copyFingerprint")}
              </button>
            )}
            {info.tlsCertificatePem && (
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
            )}
          </div>
          {showCertificate && info.tlsCertificatePem && (
            <pre className="server-info-panel__pem">{info.tlsCertificatePem}</pre>
          )}
        </section>

        <ConnectionCard
          state={connection}
          address={address}
          copied={copied === "address"}
          onCopyAddress={() => {
            void copy(address, "address");
          }}
        />

        {info.welcomeText && (
          <section className="server-info-panel__card server-info-panel__card--wide">
            <h3>{t("serverInfo.welcomeText")}</h3>
            <div
              className="server-info-panel__welcome"
              dangerouslySetInnerHTML={{ __html: info.welcomeText }}
            />
          </section>
        )}
      </div>

      <footer className="server-info-panel__footer">
        <div className="server-info-panel__footer-actions">
          <button type="button" className="button button--primary" onClick={copyDiagnostics}>
            {copied === "diagnostics" ? t("serverInfo.copied") : t("serverInfo.copyDiagnostics")}
          </button>
          <button type="button" className="button" onClick={onClose}>
            {t("serverInfo.close")}
          </button>
        </div>
        <p className="server-info-panel__privacy">{t("serverInfo.privacyNote")}</p>
      </footer>
    </Dialog>
  );
}
