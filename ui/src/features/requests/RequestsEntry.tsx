import { useTranslation } from "react-i18next";
import { useShownServerId } from "../channels";
import { usePmRequests } from "./requestsStore";
import { openRequestsView, useRequestsView } from "./requestsView";
import "./requests.css";

/** "Message requests" with its count at the top of the channel column; only while some wait. */
export function RequestsEntry() {
  const { t } = useTranslation("chat");
  const serverId = useShownServerId();
  const { incoming } = usePmRequests(serverId);
  const open = useRequestsView() !== null;
  if (serverId === null || incoming.length === 0) return null;
  return (
    <button
      type="button"
      className="requests-entry"
      aria-current={open ? "page" : undefined}
      data-testid="message-requests-entry"
      onClick={() => {
        openRequestsView(serverId);
      }}
    >
      <svg
        className="requests-entry__icon"
        viewBox="0 0 24 24"
        width="18"
        height="18"
        aria-hidden="true"
        focusable="false"
      >
        <path
          d="M3 6h18v12H3zM3 7l9 6l9-6"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinejoin="round"
        />
      </svg>
      <span className="requests-entry__label">{t("requests.entry")}</span>{" "}
      <span className="requests-entry__count" aria-hidden="true">
        {incoming.length}
      </span>
      <span className="visually-hidden">
        {t("requests.entryCount", { count: incoming.length })}
      </span>
    </button>
  );
}
