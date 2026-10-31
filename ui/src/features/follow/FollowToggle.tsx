import { useTranslation } from "react-i18next";
import { EyeIcon } from "./EyeIcon";
import type { FollowControl } from "./useFollowControl";
import "./follow.css";

/**
 * Eye in front of the channel name in the chat header (FR-3.87): filled while followed, outlined
 * otherwise. Servers before 1.4 get the FR-1.12 hint instead.
 */
export function FollowToggle({ control }: { control: FollowControl }) {
  const { t } = useTranslation("follow");
  if (!control.supported) {
    return <p className="follow-hint">{t("unsupported")}</p>;
  }
  const label = control.following ? t("header.unfollow") : t("header.follow");
  return (
    <>
      <button
        type="button"
        className="follow-toggle follow-toggle--eye"
        aria-pressed={control.following}
        aria-label={label}
        data-tooltip={label}
        disabled={control.busy}
        onClick={control.toggle}
      >
        <EyeIcon />
      </button>
      {control.error && (
        <p role="alert" className="follow-error">
          {control.error}
        </p>
      )}
    </>
  );
}
