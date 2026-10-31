import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import type { MessageRequest } from "../../api";
import { RequestCard } from "../requests/RequestCard";

export interface PendingRequest {
  serverId: number;
  request: MessageRequest;
}

/**
 * Message requests (issue #141) on top of the overview, newest first. More than one collapse to
 * "Show n more requests" unless `expanded` (the Requests filter shows them all).
 */
export function RequestsSection({
  requests,
  expanded,
}: {
  requests: readonly PendingRequest[];
  expanded: boolean;
}) {
  const { t } = useTranslation("chat");
  const [open, setOpen] = useState(false);
  const titleId = useId();
  const listId = useId();
  if (requests.length === 0) return null;
  const all = expanded || open;
  const shown = all ? requests : requests.slice(0, 1);
  const hidden = requests.length - 1;

  return (
    <section className="pm-section pm-requests" aria-labelledby={titleId}>
      <div className="pm-section__head">
        <h3 id={titleId} className="pm-section__title">
          {t("overview.requestsTitle")}
          <span className="pm-section__count">{requests.length}</span>
        </h3>
        <span className="pm-section__note">{t("overview.requestsNote")}</span>
      </div>
      <div id={listId} className="pm-requests__list">
        {shown.map(({ serverId, request }) => (
          <RequestCard
            key={`${String(serverId)}:${String(request.id)}`}
            serverId={serverId}
            request={request}
            compact
          />
        ))}
      </div>
      {!expanded && hidden > 0 && (
        <button
          type="button"
          className="pm-requests__more"
          aria-expanded={open}
          aria-controls={listId}
          onClick={() => {
            setOpen(!open);
          }}
        >
          {open ? t("overview.showFewer") : t("overview.showMore", { count: hidden })}
        </button>
      )}
    </section>
  );
}
