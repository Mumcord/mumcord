import { Fragment, useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { useServerState } from "../channels";
import { openLink } from "../chat/openLink";
import { sanitizeHtml } from "../chat/sanitize";
import { Dialog } from "../servers/Dialog";
import { useServers } from "../servers";
import { displayName } from "../servers/format";
import { channelPath, hasSavedPassword, permissionList, userCount } from "./channelModel";

interface ChannelInfoDialogProps {
  channelId: number;
  /** `mumble://` link of the channel; `null` hides "Copy link". */
  link: string | null;
  /** `null` on servers without follow support. */
  following: boolean | null;
  onClose: () => void;
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="channel-info__row">
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

/**
 * Channel info (issue #170, FR-3.89): path, description, limits, access, links, own permissions,
 * follow state and the last history clear. Every value comes from what the server sends to every
 * client or from local data; nothing is asked of the server beyond the description text.
 */
export function ChannelInfoDialog({ channelId, link, following, onClose }: ChannelInfoDialogProps) {
  const { t, i18n } = useTranslation("channelheader");
  const server = useServerState();
  const { servers } = useServers();
  const { serverId } = server;
  const [savedPassword, setSavedPassword] = useState(false);
  const [clearedAt, setClearedAt] = useState<string | null>(null);
  const [copied, setCopied] = useState<"done" | "failed" | null>(null);
  const descriptionRef = useRef<HTMLDivElement>(null);

  const channel = server.channels.get(channelId);
  const needsDescription = channel !== undefined && channel.hasDescription && !channel.description;

  useEffect(() => {
    if (serverId === null) return;
    const api = getApi();
    let live = true;
    api
      .channelPasswords(serverId)
      .then((saved) => {
        if (live) setSavedPassword(hasSavedPassword(saved, channelId));
      })
      .catch((error: unknown) => {
        console.error("Reading saved channel passwords failed", error);
      });
    api
      .historyClearedAt(serverId, channelId)
      .then((at) => {
        if (live) setClearedAt(at);
      })
      .catch((error: unknown) => {
        console.error("Reading when the history was cleared failed", error);
      });
    return () => {
      live = false;
    };
  }, [serverId, channelId]);

  // Long descriptions are only announced by hash; the text arrives as a channel update.
  useEffect(() => {
    if (serverId === null || !needsDescription) return;
    getApi()
      .channelDescriptionRequest(serverId, channelId)
      .catch((error: unknown) => {
        console.error("Fetching the channel description failed", error);
      });
  }, [serverId, channelId, needsDescription]);

  const description = channel?.description ? sanitizeHtml(channel.description) : "";

  // Links in the description open in the system browser, never inside the app window.
  useEffect(() => {
    const body = descriptionRef.current;
    if (!body) return;
    const onClick = (event: MouseEvent) => {
      const anchor = event.target instanceof Element ? event.target.closest("a") : null;
      if (!anchor) return;
      event.preventDefault();
      const href = anchor.getAttribute("href");
      if (href) {
        openLink(href).catch((error: unknown) => {
          console.error("Opening a link failed", error);
        });
      }
    };
    body.addEventListener("click", onClick);
    return () => {
      body.removeEventListener("click", onClick);
    };
  }, [description]);

  if (!channel) return null;

  const saved = servers.find((s) => s.id === serverId);
  const path = channelPath(server.channels, channelId);
  const breadcrumb = [saved ? displayName(saved) : "", ...path].filter(Boolean).join(" › ");
  const users = userCount(server.users, channelId);
  const links = channel.links
    .map((id) => channelPath(server.channels, id))
    .filter((p) => p.length > 0)
    .map((p) => p.join(" › "));
  const maxBandwidth = server.info?.maxBandwidth ?? null;

  let access = t("info.accessOpen");
  if (channel.enterRestricted) {
    access = savedPassword ? t("info.accessSaved") : t("info.accessRestricted");
  }

  const formatDate = (iso: string) =>
    new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium" }).format(new Date(iso));

  const copy = async () => {
    if (link === null) return;
    try {
      await getApi().copyText(link);
      setCopied("done");
    } catch {
      // The copy API logs the real reason; the user only needs to know it did not work.
      setCopied("failed");
    }
  };

  return (
    <Dialog
      className="channel-info"
      title={t("info.title", { channel: channel.name })}
      onClose={onClose}
    >
      <div className="channel-info__header">
        <p className="channel-info__name" aria-hidden="true">
          <span className="channel-info__hash">#</span>
          {channel.name}
        </p>
        {breadcrumb && <p className="channel-info__path">{breadcrumb}</p>}
      </div>
      {description ? (
        <div
          ref={descriptionRef}
          className="channel-info__description chat-message__body"
          dangerouslySetInnerHTML={{ __html: description }}
        />
      ) : (
        <p className="channel-info__description channel-info__description--empty">
          {needsDescription ? t("info.descriptionLoading") : t("info.noDescription")}
        </p>
      )}
      <dl className="channel-info__list">
        <Row label={t("info.users")}>
          {channel.maxUsers === null
            ? t("info.usersCount", { count: users })
            : t("info.usersOfMax", { count: users, max: channel.maxUsers })}
        </Row>
        <Row label={t("info.type")}>
          {channel.temporary ? t("info.temporary") : t("info.permanent")}
        </Row>
        <Row label={t("info.access")}>
          <span
            className={savedPassword && channel.enterRestricted ? "channel-info__ok" : undefined}
          >
            {access}
          </span>
        </Row>
        <Row label={t("info.links")}>{links.length > 0 ? links.join(", ") : t("info.none")}</Row>
        <Row label={t("info.permissions")}>
          {permissionList(channel.permissions).map(({ permission, allowed }, index) => (
            <Fragment key={permission}>
              {index > 0 && ", "}
              <span className={allowed ? undefined : "channel-info__denied"}>
                {allowed
                  ? t(`info.permission.${permission}`)
                  : t("info.denied", { permission: t(`info.permission.${permission}`) })}
              </span>
            </Fragment>
          ))}
        </Row>
        {maxBandwidth !== null && maxBandwidth > 0 && (
          <Row label={t("info.quality")}>
            {t("info.qualityLimit", { kbps: Math.round(maxBandwidth / 1000) })}
          </Row>
        )}
        {following !== null && (
          <Row label={t("info.following")}>{following ? t("info.yes") : t("info.no")}</Row>
        )}
        <Row label={t("info.cleared")}>
          {clearedAt === null ? t("info.never") : formatDate(clearedAt)}
        </Row>
      </dl>
      <div className="channel-info__footer">
        {link !== null && (
          <button type="button" className="button button--primary" onClick={() => void copy()}>
            {copied === "done"
              ? t("info.copied")
              : copied === "failed"
                ? t("copyFailed")
                : t("info.copyLink")}
          </button>
        )}
        <button type="button" className="button" onClick={onClose}>
          {t("info.close")}
        </button>
      </div>
    </Dialog>
  );
}
