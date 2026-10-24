/**
 * "You were in ‹channel› last time – Rejoin?" (QA.9): offered on the first fresh connect after
 * app start when the app quit while joined to a channel for voice. Non-blocking: ignoring it
 * keeps the read-only state; it disappears once the user joins any channel or undeafens.
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toEngineError } from "../../api";
import { useServerState } from "../channels/serverStore";
import { joinChannel, useReadOnly } from "../userpanel/readOnly";
import "./servers.css";

export function RejoinPrompt() {
  const { t } = useTranslation(["servers", "channels"]);
  const server = useServerState();
  const readOnly = useReadOnly();
  // Dismissal is per sync, so a later fresh connect with a new offer shows again.
  const [dismissed, setDismissed] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const channelId = server.rejoinChannelId;
  const channel = channelId === null ? undefined : server.channels.get(channelId);
  const visible =
    server.connected && readOnly && channel !== undefined && dismissed !== server.selfSession;

  const rejoin = (id: number) => {
    setError(null);
    joinChannel(id).catch((cause: unknown) => {
      const { code } = toEngineError(cause);
      if (code !== "permissionDenied" && code !== "notConnected") {
        console.error("Rejoining the last channel failed", code);
      }
      setError(
        code === "permissionDenied"
          ? t("channels:errors.joinDenied")
          : code === "notConnected"
            ? t("channels:errors.notConnected")
            : t("channels:errors.joinFailed"),
      );
    });
  };

  return (
    <div role="status" aria-label={t("rejoin.label")} className="servers-status-region">
      {visible && (
        <div className="servers-status" data-tone={error ? "danger" : "info"}>
          <p className="servers-status__text">
            {error ?? t("rejoin.text", { channel: channel.name })}
          </p>
          <div className="servers-status__actions">
            <button
              type="button"
              className="button button--primary"
              onClick={() => {
                rejoin(channel.id);
              }}
            >
              {t("rejoin.rejoin")}
            </button>
            <button
              type="button"
              className="button"
              onClick={() => {
                setDismissed(server.selfSession);
                setError(null);
              }}
            >
              {t("rejoin.notNow")}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
