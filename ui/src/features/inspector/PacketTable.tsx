/**
 * The packet list: one row per packet, each expandable to its decoded content. Everything
 * shown besides times, sizes and the pacer comes from other clients and is rendered as text.
 */
import { memo, useState } from "react";
import { useTranslation } from "react-i18next";
import { JsonView } from "./JsonView";
import {
  formatGap,
  formatHex,
  formatSize,
  formatTime,
  kindKey,
  packetBlock,
  peerLabel,
  type Names,
  type Packet,
} from "./packets";

interface TableProps {
  packets: readonly Packet[];
  names: Names;
}

export function PacketTable({ packets, names }: TableProps) {
  const { t } = useTranslation("settings");
  const [expanded, setExpanded] = useState<ReadonlySet<number>>(new Set());
  const toggle = (id: number) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  };
  return (
    <div className="pi-table-wrap">
      <table className="pi-table">
        <caption className="visually-hidden">{t("inspector.tableCaption")}</caption>
        <thead>
          <tr>
            <th scope="col">{t("inspector.columns.time")}</th>
            <th scope="col">{t("inspector.columns.direction")}</th>
            <th scope="col">{t("inspector.columns.type")}</th>
            <th scope="col">{t("inspector.columns.size")}</th>
            <th scope="col">{t("inspector.columns.peers")}</th>
            <th scope="col">{t("inspector.columns.pacer")}</th>
          </tr>
        </thead>
        <tbody>
          {packets.map((packet, i) => (
            <PacketRow
              key={packet.id}
              packet={packet}
              previousAtMs={packets[i - 1]?.atMs ?? null}
              names={names}
              open={expanded.has(packet.id)}
              onToggle={toggle}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
}

interface RowProps {
  packet: Packet;
  previousAtMs: number | null;
  names: Names;
  open: boolean;
  onToggle: (id: number) => void;
}

const PacketRow = memo(function PacketRow({
  packet,
  previousAtMs,
  names,
  open,
  onToggle,
}: RowProps) {
  const { t } = useTranslation("settings");
  const sent = packet.direction === "sent";
  const detailsId = `pi-details-${String(packet.id)}`;
  return (
    <>
      <tr className={`pi-row pi-row-${packet.direction}`}>
        <td className="pi-time">
          <button
            type="button"
            className="pi-expand"
            aria-expanded={open}
            aria-controls={open ? detailsId : undefined}
            aria-label={t("inspector.details", { time: formatTime(packet.atMs) })}
            onClick={() => {
              onToggle(packet.id);
            }}
          >
            <span aria-hidden="true" className="pi-chevron">
              {open ? "▾" : "▸"}
            </span>
            {formatTime(packet.atMs)}
          </button>
        </td>
        <td>
          <span className={`pi-direction pi-direction-${packet.direction}`}>
            <span aria-hidden="true">{sent ? "↑" : "↓"}</span>{" "}
            {t(sent ? "inspector.sent" : "inspector.received")}
          </span>
        </td>
        <td>
          <span className="pi-kind">{t(`inspector.kinds.${kindKey(packet)}`)}</span>{" "}
          <code className="pi-kind-id">{packet.kind}</code>
        </td>
        <td className="pi-size">{formatSize(packet.size)}</td>
        <td className="pi-peers">
          <Peers packet={packet} names={names} />
        </td>
        <td className="pi-pacer">
          <Pacer packet={packet} />
        </td>
      </tr>
      {open && (
        <tr className="pi-details-row">
          <td colSpan={6} id={detailsId}>
            <PacketDetails packet={packet} previousAtMs={previousAtMs} names={names} />
          </td>
        </tr>
      )}
    </>
  );
});

function Peers({ packet, names }: { packet: Packet; names: Names }) {
  const { t } = useTranslation("settings");
  if (packet.direction === "received") {
    if (packet.sender === null) return null;
    return <span>← {peerLabel(names, packet.serverId, packet.sender)}</span>;
  }
  const [only] = packet.receivers;
  if (packet.receivers.length === 1 && only !== undefined) {
    return <span>→ {peerLabel(names, packet.serverId, only)}</span>;
  }
  if (packet.receivers.length === 0) return null;
  return (
    <details className="pi-receivers">
      <summary>{t("inspector.toUsers", { count: packet.receivers.length })}</summary>
      <ul>
        {packet.receivers.map((session) => (
          <li key={session}>{peerLabel(names, packet.serverId, session)}</li>
        ))}
      </ul>
    </details>
  );
}

function Pacer({ packet }: { packet: Packet }) {
  const { t } = useTranslation("settings");
  if (packet.delayMs === null) return null;
  return (
    <span className={packet.delayMs > 0 ? "pi-delayed" : undefined}>
      {packet.delayMs > 0
        ? t("inspector.delayed", { ms: packet.delayMs })
        : t("inspector.notDelayed")}
      {" · "}
      {t("inspector.tokensLeft", { tokens: packet.tokensLeft ?? 0 })}
    </span>
  );
}

function PacketDetails({ packet, previousAtMs, names }: Omit<RowProps, "open" | "onToggle">) {
  const { t } = useTranslation("settings");
  const [hex, setHex] = useState(false);
  const [copy, setCopy] = useState<"copied" | "failed" | null>(null);
  const copyPacket = async () => {
    try {
      await navigator.clipboard.writeText(packetBlock(packet, names, previousAtMs).join("\n"));
      setCopy("copied");
    } catch (error: unknown) {
      console.error("Copying the packet failed", error);
      setCopy("failed");
    }
  };
  return (
    <div className="pi-details">
      <dl className="pi-facts">
        <dt>{t("inspector.fields.gap")}</dt>
        <dd>{previousAtMs === null ? "–" : formatGap(packet.atMs - previousAtMs)}</dd>
        <dt>{t("inspector.fields.kind")}</dt>
        <dd>
          <code>{packet.kind}</code>
        </dd>
        <dt>{t("inspector.fields.size")}</dt>
        <dd>{t("inspector.bytes", { bytes: packet.size })}</dd>
      </dl>
      {packet.decoded !== null ? (
        <JsonView json={packet.decoded} label={t("inspector.fields.decoded")} />
      ) : (
        <p className="settings-hint">
          {packet.pluginData ? t("inspector.noDecoding") : t("inspector.controlOnly")}
        </p>
      )}
      <div className="settings-actions">
        {packet.rawHex !== "" && (
          <button
            type="button"
            className="settings-button"
            aria-expanded={hex}
            onClick={() => {
              setHex(!hex);
            }}
          >
            {t(hex ? "inspector.hideHex" : "inspector.showHex")}
          </button>
        )}
        <button type="button" className="settings-button" onClick={() => void copyPacket()}>
          {t("inspector.copy")}
        </button>
        <span role="status" className={copy === "failed" ? "settings-error" : "settings-success"}>
          {copy === null ? "" : t(copy === "copied" ? "inspector.copied" : "inspector.copyFailed")}
        </span>
      </div>
      {hex && (
        // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
        <pre className="pi-code" tabIndex={0} aria-label={t("inspector.rawHex")}>
          {formatHex(packet.rawHex)}
        </pre>
      )}
    </div>
  );
}
