import { useTranslation } from "react-i18next";
import { useOpenPrivacySettings } from "../../app/view";

/**
 * Stands in for the player and playlist while "Show link previews and embeds" is off: the watch
 * party loads YouTube, which that switch forbids, so nothing is requested until it is turned on.
 */
export function EmbedsOffNotice() {
  const { t } = useTranslation("watchparty");
  const openPrivacy = useOpenPrivacySettings();
  return (
    <div className="watch-party__embeds-off" role="status">
      <p>{t("embedsOffJoin")}</p>
      {openPrivacy !== null && (
        <button type="button" className="button" onClick={openPrivacy}>
          {t("openPrivacy")}
        </button>
      )}
    </div>
  );
}
