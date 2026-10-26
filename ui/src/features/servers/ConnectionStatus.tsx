import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ActionContext, ServersState } from "./store";
import {
  cancelReconnect,
  disconnect,
  dismissActionError,
  openDialog,
  requestConnect,
  useServers,
} from "./store";
import { EmptyState } from "./EmptyState";
import { EDIT_FIXES, connectionErrorText, serverLimitText } from "./errors";
import { displayName } from "./format";
import "./servers.css";

/** Seconds until the next reconnect attempt, counted down locally between engine updates. */
function useCountdown(seconds: number, key: unknown): number {
  const [left, setLeft] = useState(seconds);
  const [lastKey, setLastKey] = useState(key);
  if (key !== lastKey) {
    setLastKey(key);
    setLeft(seconds);
  }
  useEffect(() => {
    const timer = setInterval(() => {
      setLeft((current) => Math.max(current - 1, 0));
    }, 1000);
    return () => {
      clearInterval(timer);
    };
  }, [key]);
  return left;
}

type Tone = "info" | "warning" | "danger";

interface BarProps {
  tone: Tone;
  text: string;
  /** Spoken instead of `text` when the visible text changes every second (countdown). */
  spokenText?: string;
  children?: ReactNode;
}

function Bar({ tone, text, spokenText, children }: BarProps) {
  return (
    <div className="servers-status" data-tone={tone}>
      <p className="servers-status__text">
        {spokenText === undefined ? (
          text
        ) : (
          <>
            <span aria-hidden="true">{text}</span>
            <span className="visually-hidden">{spokenText}</span>
          </>
        )}
      </p>
      {children && <div className="servers-status__actions">{children}</div>}
    </div>
  );
}

function nameOf(state: ServersState, id: number): string {
  const server = state.servers.find((s) => s.id === id);
  return server ? displayName(server) : "";
}

function Reconnecting({ state }: { state: ServersState }) {
  const { t } = useTranslation("servers");
  const connection = state.connection;
  const reconnecting = connection.state === "reconnecting" ? connection : null;
  const left = useCountdown(reconnecting?.nextRetrySecs ?? 0, reconnecting);
  if (!reconnecting) return null;
  const text = (seconds: number) =>
    t("status.reconnecting", {
      attempt: reconnecting.attempt,
      max: reconnecting.maxAttempts,
      seconds,
    });
  // The live region only hears about each attempt, not every tick of the countdown.
  return (
    <Bar tone="warning" text={text(left)} spokenText={text(reconnecting.nextRetrySecs)}>
      <button type="button" className="button" onClick={() => void cancelReconnect()}>
        {t("status.cancel")}
      </button>
    </Bar>
  );
}

const ACTION_KEYS: Record<ActionContext, string> = {
  reorder: "status.actionFailed.reorder",
  remove: "status.actionFailed.remove",
  disconnect: "status.actionFailed.disconnect",
  alwaysConnect: "status.actionFailed.alwaysConnect",
  notifications: "status.actionFailed.notifications",
  savePassword: "status.actionFailed.savePassword",
};

/** Thin bar above the chat (spec §5.2). Shows nothing while connected normally or idle. */
function StatusBar({ state }: { state: ServersState }) {
  const { t } = useTranslation(["servers", "common"]);
  const { connection, ended, actionError } = state;

  if (actionError) {
    return (
      <Bar
        tone="danger"
        text={
          actionError.error.code === "serverLimitReached"
            ? serverLimitText(t, state.serverLimit, actionError.context === "alwaysConnect")
            : t(ACTION_KEYS[actionError.context])
        }
      >
        <button type="button" className="button" onClick={dismissActionError}>
          {t("common:close")}
        </button>
      </Bar>
    );
  }
  switch (connection.state) {
    case "connecting":
      return (
        <Bar
          tone="info"
          text={t("status.connecting", { server: nameOf(state, connection.serverId) })}
        >
          <button type="button" className="button" onClick={() => void disconnect()}>
            {t("status.cancel")}
          </button>
        </Bar>
      );
    case "reconnecting":
      return <Reconnecting state={state} />;
    case "connected":
      // A refusal for another server (e.g. over the server limit) must not be hidden by the
      // shown server being connected.
      if (ended) break;
      return state.transport === "tcp" ? <Bar tone="info" text={t("status.tcp")} /> : null;
    case "disconnected":
      break;
  }
  if (!ended) return null;
  const reason = ended.error ? connectionErrorText(t, ended.error, state.serverLimit) : null;
  const text = ended.afterLoss
    ? reason
      ? t("status.disconnectedReason", { reason })
      : t("status.disconnected")
    : t("status.failed", { server: nameOf(state, ended.serverId), reason: reason ?? "" });
  return (
    <Bar tone="danger" text={text}>
      {ended.error && EDIT_FIXES.includes(ended.error.code) && (
        <button
          type="button"
          className="button"
          onClick={() => {
            openDialog({ kind: "edit", serverId: ended.serverId });
          }}
        >
          {t("status.editServer")}
        </button>
      )}
      <button
        type="button"
        className="button button--primary"
        onClick={() => {
          requestConnect(ended.serverId);
        }}
      >
        {ended.afterLoss ? t("status.reconnect") : t("common:retry")}
      </button>
    </Bar>
  );
}

/**
 * Content of the chat status slot: the connection bar, or the first-server panel. The status
 * region stays mounted (empty when idle) so screen readers pick up every change.
 */
export function ConnectionStatus() {
  const { t } = useTranslation("servers");
  const state = useServers();
  const empty = state.ready && state.servers.length === 0;
  return (
    <>
      <div role="status" aria-label={t("status.label")} className="servers-status-region">
        {state.ready && !empty && <StatusBar state={state} />}
      </div>
      {empty && <EmptyState />}
    </>
  );
}
