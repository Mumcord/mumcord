import { useId, useRef, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import type { MessageRequest } from "../../api";
import { Avatar } from "../members/Avatar";
import { RequestCard } from "./RequestCard";
import { usePmRequests } from "./requestsStore";
import { selectRequest } from "./requestsView";
import "../chat/chat.css";
import "./requests.css";

/**
 * "Message requests" in place of the chat (issue #141): one tab per held request, the selected
 * one below. Replying stays blocked until a request is accepted.
 */
export function RequestsView({
  serverId,
  selectedId,
}: {
  serverId: number;
  selectedId: number | null;
}) {
  const { t } = useTranslation("chat");
  const { incoming } = usePmRequests(serverId);
  const selected = incoming.find((r) => r.id === selectedId) ?? incoming[0];
  const baseId = useId();
  const tabsRef = useRef<HTMLDivElement>(null);
  const tabId = (request: MessageRequest) => `${baseId}-tab-${String(request.id)}`;
  const panelId = `${baseId}-panel`;

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (!selected) return;
    const at = incoming.indexOf(selected);
    let next: number;
    switch (event.key) {
      case "ArrowRight":
      case "ArrowDown":
        next = at + 1;
        break;
      case "ArrowLeft":
      case "ArrowUp":
        next = at - 1;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = incoming.length - 1;
        break;
      default:
        return;
    }
    event.preventDefault();
    const target = incoming[(next + incoming.length) % incoming.length];
    if (!target) return;
    selectRequest(target.id);
    tabsRef.current?.querySelector<HTMLElement>(`#${CSS.escape(tabId(target))}`)?.focus();
  };

  return (
    <div className="chat requests">
      <header className="chat-header">
        <h2 className="chat-header__title">{t("requests.title")}</h2>
      </header>
      {selected === undefined ? (
        <p className="chat__none">{t("requests.empty")}</p>
      ) : (
        <>
          <div
            ref={tabsRef}
            role="tablist"
            aria-label={t("requests.listLabel")}
            className="requests-tabs"
          >
            {incoming.map((request) => {
              const active = request === selected;
              return (
                <button
                  key={request.id}
                  id={tabId(request)}
                  type="button"
                  role="tab"
                  aria-selected={active}
                  aria-controls={panelId}
                  tabIndex={active ? 0 : -1}
                  className="requests-tab"
                  onKeyDown={onKeyDown}
                  onClick={() => {
                    selectRequest(request.id);
                  }}
                >
                  <Avatar name={request.name} size="sm" />
                  <span className="requests-tab__name">{request.name}</span>
                  <span className="requests-tab__tag">
                    {request.chatRequest
                      ? t("requests.chatRequestTag")
                      : t("requests.messageRequestTag")}
                  </span>
                </button>
              );
            })}
          </div>
          <div
            id={panelId}
            role="tabpanel"
            aria-labelledby={tabId(selected)}
            className="requests-panel"
          >
            <RequestCard key={selected.id} serverId={serverId} request={selected} />
          </div>
          <p className="requests-composer">{t("requests.replyBlocked", { name: selected.name })}</p>
        </>
      )}
    </div>
  );
}
