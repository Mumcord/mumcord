import { useTranslation } from "react-i18next";
import { getApi, type FileCard, type OwnShare, type ShareEndReason } from "../../../api";
import { CardHead, HashRow } from "./CardParts";
import { useEndsText } from "./useEndsText";
import { CheckIcon, ClockIcon, DownloadIcon, StopIcon } from "./fileIcons";
import { formatBytes } from "./fileFormat";

type EndKey =
  "sender.endedStopped" | "sender.endedExpired" | "sender.endedLeft" | "sender.endedChanged";

const END_REASON_KEY: Record<ShareEndReason, EndKey | null> = {
  stopped: "sender.endedStopped",
  expired: "sender.endedExpired",
  leftServer: "sender.endedLeft",
  fileChanged: "sender.endedChanged",
  // Quitting has no own text; the card just says the sharing ended.
  quit: null,
};

/** The sender's own card (FR-7.16, mockup 5): who has it, who is getting it, Stop sharing. */
export function OwnFileCard({ card }: { card: FileCard }) {
  const { t, i18n } = useTranslation("files");
  const share = card.share;
  const ended = share?.ended ?? false;
  const ends = useEndsText(card.expires);
  const stop = () => {
    getApi()
      .filesStop(card.shareId)
      .catch((cause: unknown) => {
        console.error("Stopping the share failed", cause);
      });
  };

  const endReason = share?.endReason ?? null;
  const reasonKey = endReason === null ? null : END_REASON_KEY[endReason];

  return (
    <div
      className="audio-card file-card"
      data-state={ended ? "gone" : "own"}
      role="group"
      aria-label={t("card.labelOwn", {
        name: card.name,
        size: formatBytes(card.size, i18n.language),
        state: ended ? t("sender.ended") : t("card.state.available"),
      })}
    >
      <CardHead card={card} />
      {share !== null && <Peers share={share} />}
      {!ended && <HashRow sha256={card.sha256} />}
      {ended ? (
        <p className="file-card__status">
          <ClockIcon size={14} />
          <span>
            {t("sender.ended")}
            {reasonKey !== null && ` · ${t(reasonKey)}`}
          </span>
        </p>
      ) : (
        <>
          <p className="file-card__status">
            <span className="file-card__dot" data-online="" />
            {t("sender.sharing", { when: ends })}
          </p>
          <div className="file-card__actions">
            <button
              type="button"
              className="media-pill file-card__button file-card__button--danger"
              onClick={stop}
            >
              <StopIcon size={14} />
              <span>{t("sender.stop")}</span>
            </button>
          </div>
        </>
      )}
    </div>
  );
}

/** "Downloaded by Bob and Carol" and "1 download in progress · Dave 40 % · 2 waiting". */
function Peers({ share }: { share: OwnShare }) {
  const { t, i18n } = useTranslation("files");
  const names = new Intl.ListFormat(i18n.language, { type: "conjunction" }).format(
    share.downloadedBy,
  );
  const waiting = share.waiting > 0 ? t("sender.waiting", { count: share.waiting }) : null;
  const sending = share.sendingTo;
  let progress: string | null = null;
  if (sending !== null) {
    const details = [
      `${sending.recipient} ${t("card.progressValue", { percent: sending.percent })}`,
      waiting,
    ]
      .filter((part) => part !== null)
      .join(" · ");
    progress = t("sender.inProgress", { count: 1, details });
  } else if (waiting !== null) {
    progress = waiting;
  }
  if (share.downloadedBy.length === 0 && progress === null) return null;
  return (
    <div className="file-card__peers">
      {share.downloadedBy.length > 0 && (
        <p>
          <span className="file-card__ok">
            <CheckIcon size={14} />
            {t("sender.downloadedBy", { names })}
          </span>
        </p>
      )}
      {progress !== null && (
        <p>
          <DownloadIcon size={14} />
          {progress}
        </p>
      )}
    </div>
  );
}
