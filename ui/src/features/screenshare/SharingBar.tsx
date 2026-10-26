/**
 * Red bar above the user panel while the user shares their screen (FR-2.71): viewer count and an
 * "End" button. It cannot be dismissed; it goes away when the share ends. The status role makes
 * screen readers announce it when it appears.
 */
import { useTranslation } from "react-i18next";
import { useOwnShare } from "./useOwnShare";
import { useShareActions } from "./useShareActions";
import "./screenShare.css";

export function SharingBar() {
  const { t } = useTranslation("screenshare");
  const share = useOwnShare();
  const { stop } = useShareActions(share);
  if (share.own === null) return null;
  return (
    <div className="sharing-bar" role="status">
      <span className="sharing-bar__text">{t("sharingBar")}</span>
      <span className="sharing-bar__viewers">
        {t("barViewers", { count: share.own.viewers.length })}
      </span>
      <button
        type="button"
        className="sharing-bar__end"
        aria-label={t("barEndLabel")}
        onClick={stop}
      >
        {t("barEnd")}
      </button>
    </div>
  );
}
