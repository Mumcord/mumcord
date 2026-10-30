/**
 * Card under a message for a `mumble://` link (issue #175): the server, the channel, the live
 * figures the engine queried, the credential warning, and Join / Copy link / Dismiss. Nothing here
 * is HTML — the link comes from an untrusted message and the figures from an untrusted server
 * answer, so React renders each value as text. Credentials are never shown, only the fact that the
 * link carries them.
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { getApi, type MumbleLinkStats } from "../../api";
import { useServerStateOf } from "../channels";
import { useServers } from "../servers";
import {
  canonicalLink,
  joinText,
  linkHasCredentials,
  parseMumbleLink,
  savedServer,
} from "./mumbleLink";
import { loadMumbleStats } from "./links";
import { DismissButton } from "./cardButtons";

interface MumbleCardProps {
  url: string;
  /** Links of this message that carry credentials, for the warning's count. */
  credentialLinks: number;
  /** A preview of a link the user has typed but not sent: no buttons, nothing to act on. */
  preview?: boolean;
  tabIndex: number;
  onDismiss?: () => void;
}

export function MumbleLinkCard({
  url,
  credentialLinks,
  preview = false,
  tabIndex,
  onDismiss,
}: MumbleCardProps) {
  const { t } = useTranslation("chat");
  const { servers } = useServers();
  const [stats, setStats] = useState<MumbleLinkStats | null>(null);
  const [joined, setJoined] = useState(false);

  useEffect(() => {
    let active = true;
    void loadMumbleStats(url).then((answer) => {
      if (active) setStats(answer);
    });
    return () => {
      active = false;
    };
  }, [url]);

  const link = parseMumbleLink(url);
  // A link to a server you have saved reads with its saved name, like the server bar (issue #175).
  const saved = link === null ? null : savedServer(servers, link);
  const connected = useServerStateOf(saved?.id ?? null).connected;
  if (link === null) return null;

  const server = saved?.label ?? link.address;
  const channel = link.channelPath.join(" / ");
  const hasCredentials = linkHasCredentials(url);

  const join = () => {
    void getApi()
      .mumbleLinkJoin(url)
      .then((outcome) => {
        setJoined(outcome.kind === "connecting" || outcome.kind === "shown");
      })
      .catch((error: unknown) => {
        console.error("Joining a chat link failed", error);
      });
  };

  const copy = () => {
    // The canonical link, so the copied text carries no credentials.
    void getApi()
      .copyText(canonicalLink(url))
      .catch((error: unknown) => {
        console.error("Copying a chat link failed", error);
      });
  };

  const checkAgain = () => {
    void loadMumbleStats(url, true).then((answer) => {
      setStats(answer);
    });
  };

  return (
    <article className="mumble-card" aria-label={t("mumbleCard.label", { server })}>
      <p className="mumble-card__server">{server}</p>
      {channel !== "" && (
        <p className="mumble-card__channel">{t("mumbleCard.channel", { channel })}</p>
      )}
      {hasCredentials && (
        <p className="mumble-card__warning">
          {t("mumbleCard.warning", { count: credentialLinks })}
        </p>
      )}
      <p className="mumble-card__figures">
        {stats === null ? t("mumbleCard.querying") : figuresText(t, stats)}
        {stats?.kind === "unreachable" && (
          <button
            type="button"
            className="mumble-card__refresh"
            tabIndex={tabIndex}
            aria-label={t("mumbleCard.retryLabel", { server })}
            onClick={checkAgain}
          >
            {t("mumbleCard.retry")}
          </button>
        )}
      </p>
      {preview ? null : (
        <div className="mumble-card__actions">
          <button
            type="button"
            className="mumble-card__join"
            tabIndex={tabIndex}
            aria-label={t("mumbleCard.joinLabel", { server })}
            onClick={join}
          >
            {joined ? t("mumbleCard.joined") : joinText(t, saved, connected)}
          </button>
          <button
            type="button"
            className="mumble-card__copy"
            tabIndex={tabIndex}
            aria-label={t("mumbleCard.copyLabel", { server })}
            onClick={copy}
          >
            {t("mumbleCard.copy")}
          </button>
        </div>
      )}
      {preview || onDismiss === undefined ? null : (
        <DismissButton onDismiss={onDismiss} tabIndex={tabIndex} />
      )}
    </article>
  );
}

/** The figures, or the reason there are none. */
function figuresText(t: TFunction<"chat">, stats: MumbleLinkStats): string {
  switch (stats.kind) {
    case "answered":
      return t("mumbleCard.figures", {
        ping: stats.pingMs,
        users: stats.users,
        max: stats.maxUsers,
        version: stats.version,
      });
    case "privacyOff":
      return t("mumbleCard.privacyOff");
    case "unreachable":
      return t("mumbleCard.unreachable");
  }
}
