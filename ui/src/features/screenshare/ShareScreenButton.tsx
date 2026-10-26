/**
 * "Share screen" (FR-2.42, FR-2.65, Beta). Disabled with its reason as tooltip and description
 * while sharing isn't possible; turns into "Stop sharing" while the user shares. Closing the
 * system picker without choosing is no error: no share appears and the button is back.
 */
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { BetaBadge } from "../../components/BetaBadge";
import type { OwnShareState } from "./useOwnShare";
import { useShareActions } from "./useShareActions";

export function ShareScreenButton({ share }: { share: OwnShareState }) {
  const { t } = useTranslation("screenshare");
  const { own, block, recheck } = share;
  const { starting, failed, disabled, start, stop } = useShareActions(share);
  const reasonId = useId();
  const errorId = useId();

  if (own !== null) {
    return (
      <>
        <button
          type="button"
          className="button share-button share-button--stop"
          aria-describedby={failed ? errorId : undefined}
          onClick={stop}
        >
          {t("stop")}
        </button>
        {failed && (
          <p id={errorId} className="share-button__error" role="alert">
            {t(failed)}
          </p>
        )}
      </>
    );
  }

  const reason = block === null ? null : t(`blocked.${block}`);
  const describedBy = [reason && reasonId, failed && errorId].filter(Boolean).join(" ");
  return (
    <>
      <button
        type="button"
        className="button share-button"
        aria-disabled={disabled || undefined}
        aria-busy={starting || undefined}
        aria-describedby={describedBy || undefined}
        data-tooltip={reason ?? t("shareHint")}
        onFocus={() => void recheck()}
        onPointerEnter={() => void recheck()}
        onClick={() => void start()}
      >
        {starting ? t("choosing") : t("share")}
        <BetaBadge />
      </button>
      {reason && (
        <span id={reasonId} className="visually-hidden">
          {reason}
        </span>
      )}
      {failed && (
        <p id={errorId} className="share-button__error" role="alert">
          {t(failed)}
        </p>
      )}
    </>
  );
}
