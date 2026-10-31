import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ConnectionState } from "../../api";
import { useConnectionStats } from "../connection/useConnectionStats";

export interface ServerSectionHeaderProps {
  name: string;
  connection: ConnectionState["state"];
  /** People on the server right now. */
  online: number;
  collapsed: boolean;
  onToggle: () => void;
  /** Id of the element the toggle shows and hides (the tree). */
  controls: string;
  /** The server's ⋯ menu, if any. */
  menu?: ReactNode;
  /** Controls at the row's end, e.g. "Jump to my channel". */
  end?: ReactNode;
}

/**
 * Section header of the shown server above its channel tree (issue #172, mockup item 6): a
 * collapse chevron, a connection dot, the name in capitals, "N online · XX ms" and the ⋯ menu.
 */
export function ServerSectionHeader(props: ServerSectionHeaderProps) {
  const { t } = useTranslation("channels");
  // The same smoothed ping as the connection info; polled only while connected.
  const stats = useConnectionStats(props.connection === "connected");
  const ping = stats.status === "ready" ? stats.stats.rttMs : null;
  const meta =
    ping === null
      ? t("section.online", { count: props.online })
      : t("section.onlinePing", { count: props.online, ping: Math.round(ping) });

  return (
    <div className="server-section">
      <h2 className="server-section__title">
        <button
          type="button"
          className="server-section__toggle"
          aria-expanded={!props.collapsed}
          aria-controls={props.controls}
          onClick={props.onToggle}
        >
          <svg
            className="server-section__chevron"
            data-expanded={!props.collapsed}
            viewBox="0 0 24 24"
            width="14"
            height="14"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
            focusable="false"
          >
            <path d="m9 6 6 6-6 6" />
          </svg>
          <span
            className="server-section__dot"
            data-state={props.connection}
            role="img"
            aria-label={t(`section.state.${props.connection}`)}
          />
          <span className="server-section__name">{props.name}</span>
        </button>
      </h2>
      {props.connection === "connected" && <span className="server-section__meta">{meta}</span>}
      {props.connection === "reconnecting" && (
        <span className="server-section__meta">{t("section.state.reconnecting")}</span>
      )}
      {props.menu}
      {props.end}
    </div>
  );
}
