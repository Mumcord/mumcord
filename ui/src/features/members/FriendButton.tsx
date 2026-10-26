import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toEngineError, type User } from "../../api";
import { setFriend, useFriend } from "../friends";

/**
 * Heart toggle on the pinned person card (FR-1.64). People without a certificate can't be
 * recognised again, so the button stays disabled and says why.
 */
export function FriendButton({ user, serverId }: { user: User; serverId?: number | undefined }) {
  const { t } = useTranslation("friends");
  const friend = useFriend(user.certHash);
  const [failed, setFailed] = useState(false);
  const noCertificate = user.certHash === null;
  const label = noCertificate ? t("noCertificate") : friend ? t("remove") : t("add");
  return (
    <>
      <button
        type="button"
        className="profile-preview__icon"
        aria-label={label}
        data-tooltip={label}
        aria-pressed={friend !== undefined}
        aria-disabled={noCertificate}
        onClick={() => {
          if (noCertificate) return;
          setFailed(false);
          setFriend(user, friend === undefined, serverId).catch((cause: unknown) => {
            console.error("Changing a friend failed", toEngineError(cause).code);
            setFailed(true);
          });
        }}
      >
        <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" focusable="false">
          <path
            d="M12 21s-7-4.5-9.5-9A5.5 5.5 0 0 1 12 6a5.5 5.5 0 0 1 9.5 6c-2.5 4.5-9.5 9-9.5 9z"
            fill={friend ? "currentColor" : "none"}
            stroke="currentColor"
            strokeWidth="2"
            strokeLinejoin="round"
          />
        </svg>
      </button>
      {failed && (
        <span role="alert" className="visually-hidden">
          {t("failed")}
        </span>
      )}
    </>
  );
}
