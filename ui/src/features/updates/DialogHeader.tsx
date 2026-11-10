import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { UpdateChannel } from "../../api";

/** Logo, title and one line below it; shared by every state of the update dialog. */
export function DialogHeader(props: {
  titleId: string;
  title: string;
  subtitle: string;
  badge?: ReactNode;
}) {
  return (
    <header className="upd-header">
      <img className="upd-logo" src={`${import.meta.env.BASE_URL}favicon.png`} alt="" />
      <div className="upd-header__text">
        <h2 id={props.titleId} className="upd-title">
          {props.title}
        </h2>
        <p className="upd-subtitle">{props.subtitle}</p>
      </div>
      {props.badge}
    </header>
  );
}

export function Badge({ children, rc = false }: { children: ReactNode; rc?: boolean }) {
  return <span className={`upd-badge${rc ? " upd-badge--rc" : ""}`}>{children}</span>;
}

/** The offered version's channel: "Stable", or an amber "Release candidate". */
export function ChannelBadge({ channel }: { channel: UpdateChannel }) {
  const { t } = useTranslation("appwindow");
  return channel === "releaseCandidates" ? (
    <Badge rc>{t("updater.releaseCandidate")}</Badge>
  ) : (
    <Badge>{t("updater.stable")}</Badge>
  );
}

/** Warning under the release notes of a pre-release offer. */
export function PrereleaseNote() {
  const { t } = useTranslation("appwindow");
  return (
    <p className="upd-prerelease">
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0zM12 9v4M12 17h.01" />
      </svg>
      <span>{t("updater.prereleaseNote")}</span>
    </p>
  );
}
