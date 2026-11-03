import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { announce } from "../../../a11y/announcer";
import { useTranslation } from "react-i18next";
import { EngineError, getApi, type DownloadBlock, type FileCard } from "../../../api";
import { shownServerId } from "../../channels/serverStore";
import { openSettingsSection } from "../../settings/settingsOpener";
import { DownloadConfirmDialog } from "../dialogs/DownloadConfirmDialog";
import { RiskyFileDialog } from "../dialogs/RiskyFileDialog";
import { CardHead, HashRow } from "./CardParts";
import { useEndsText } from "./useEndsText";
import {
  BlockedIcon,
  ClockIcon,
  CloseIcon,
  DownloadIcon,
  FolderIcon,
  DocumentIcon,
  LockIcon,
  WarningIcon,
} from "./fileIcons";
import { folderOf, formatBytes, maxSizeLabel, progressParts } from "./fileFormat";
import { failReasonText, filesUnavailableText } from "./fileTexts";

type Dialog = "confirm" | "risky" | "riskyOpen" | null;

/** The `card.state.*` word for the accessible label and the announcement. */
function stateWord(card: FileCard): string {
  const kind = card.state.kind;
  if (kind === "requested") return "queued";
  return kind;
}

/** The look of the card's left edge and icon (mock.css of #348). */
function edge(card: FileCard, dismissed: boolean): string | undefined {
  switch (card.state.kind) {
    case "done":
      return "done";
    case "mismatch":
      return dismissed ? undefined : "mismatch";
    case "unavailable":
      return "gone";
    default:
      return undefined;
  }
}

/** A recipient's file card (FR-7.19 – 7.22, 7.26, 7.28, 7.29; mockups 2 – 4, 6, 7, 12, 13). */
export function ReceivedFileCard({ card }: { card: FileCard }) {
  const { t, i18n } = useTranslation("files");
  const [dialog, setDialog] = useState<Dialog>(null);
  // "Dismiss" on a mismatch only hides the alert here; the card offers Download again.
  const [dismissed, setDismissed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const state = card.state;
  const showMismatch = state.kind === "mismatch" && !dismissed;
  const word = t(`card.state.${stateWord(card)}`);
  // Speaks state changes only, not every card in the history as it scrolls into view.
  const spoken = useRef(word);
  useEffect(() => {
    if (spoken.current === word) return;
    spoken.current = word;
    announce(`${card.name}: ${word}`);
  }, [word, card.name]);

  const run = (action: (serverId: number) => Promise<unknown>, what: string) => {
    const serverId = shownServerId();
    setError(null);
    action(serverId).catch((cause: unknown) => {
      console.error(`${what} failed`, cause);
      if (cause instanceof EngineError && cause.code === "notFound") {
        setError(t("card.gone", { folder: state.kind === "done" ? folderOf(state.path) : "" }));
      } else {
        setError(
          t("card.failed", { reason: cause instanceof Error ? cause.message : String(cause) }),
        );
      }
    });
  };
  const download = () => {
    setDialog(null);
    setDismissed(false);
    run((serverId) => getApi().filesDownload(serverId, card.shareId), "Starting the download");
  };
  const open = () => {
    setDialog(null);
    run((serverId) => getApi().filesOpen(serverId, card.shareId), "Opening the file");
  };
  const askDownload = () => {
    setDialog(card.risky ? "risky" : "confirm");
  };

  const label = t("card.label", {
    name: card.name,
    size: formatBytes(card.size, i18n.language),
    sender: card.senderName,
    state: word,
  });

  return (
    <div
      className="audio-card file-card"
      data-state={edge(card, dismissed)}
      data-risky={card.risky || undefined}
      role="group"
      aria-label={label}
    >
      <CardHead card={card} />
      <Body
        card={card}
        dismissed={dismissed}
        onDownload={askDownload}
        onCancel={() => {
          run((serverId) => getApi().filesCancel(serverId, card.shareId), "Cancelling");
        }}
        onOpen={() => {
          if (card.risky) setDialog("riskyOpen");
          else open();
        }}
        onOpenFolder={() => {
          run((serverId) => getApi().filesOpenFolder(serverId, card.shareId), "Opening the folder");
        }}
      />
      {error !== null && (
        <p className="file-card__error" role="alert">
          {error}
        </p>
      )}
      {showMismatch && (
        <div className="file-card__actions">
          <button
            type="button"
            className="media-pill file-card__button"
            onClick={() => {
              setDismissed(true);
            }}
          >
            {t("card.dismiss")}
          </button>
        </div>
      )}
      {dialog === "confirm" && (
        <DownloadConfirmDialog
          card={card}
          onConfirm={download}
          onClose={() => {
            setDialog(null);
          }}
        />
      )}
      {(dialog === "risky" || dialog === "riskyOpen") && (
        <RiskyFileDialog
          card={card}
          action={dialog === "risky" ? "download" : "open"}
          onConfirm={dialog === "risky" ? download : open}
          onClose={() => {
            setDialog(null);
          }}
        />
      )}
    </div>
  );
}

interface BodyProps {
  card: FileCard;
  dismissed: boolean;
  onDownload: () => void;
  onCancel: () => void;
  onOpen: () => void;
  onOpenFolder: () => void;
}

function Body({ card, dismissed, onDownload, onCancel, onOpen, onOpenFolder }: BodyProps) {
  const { t } = useTranslation("files");
  const { t: tP2p } = useTranslation("p2p");
  const state = card.state;
  const sender = card.senderName;

  switch (state.kind) {
    case "available":
    case "cancelled":
      return (
        <Offer card={card} onDownload={onDownload}>
          {state.kind === "cancelled" && <p className="file-card__status">{t("card.cancelled")}</p>}
        </Offer>
      );
    case "failed": {
      const text =
        state.reason.kind === "busy" || state.reason.kind === "noSpace"
          ? failReasonText(t, tP2p, state.reason, sender)
          : t("card.failed", { reason: failReasonText(t, tP2p, state.reason, sender) });
      return (
        <Offer card={card} onDownload={onDownload} retry>
          <p className="file-card__status file-card__problem">
            <WarningIcon size={14} />
            <span>{text}</span>
          </p>
        </Offer>
      );
    }
    case "mismatch":
      return dismissed ? (
        <Offer card={card} onDownload={onDownload} />
      ) : (
        <div className="file-card__alert" role="alert">
          <span className="file-card__alert-icon" aria-hidden="true">
            <WarningIcon size={18} />
          </span>
          <div>
            <p className="file-card__alert-title">{t("card.mismatchTitle")}</p>
            <p>{t("card.mismatch", { sender })}</p>
          </div>
        </div>
      );
    case "unavailable":
      if (state.reason === "senderOffline") {
        return (
          <>
            <p className="file-card__status">
              <span className="file-card__dot" />
              {t("card.unavailable.offline", { sender })}
            </p>
            <div className="file-card__actions">
              <DownloadButton blocked onClick={onDownload} />
            </div>
          </>
        );
      }
      return (
        <>
          <p className="file-card__status">
            <ClockIcon size={14} />
            {t("card.unavailable.ended")}
          </p>
          <p className="file-card__muted">{t("card.unavailable.askAgain", { sender })}</p>
        </>
      );
    case "requested":
    case "queued":
      return (
        <>
          <HashRow sha256={card.sha256} />
          <p className="file-card__status">
            {t("card.queued", { position: state.kind === "queued" ? state.position : 1 })}
          </p>
          <CancelRow onCancel={onCancel} />
        </>
      );
    case "downloading":
      return (
        <>
          <Progress card={card} received={state.received} bytesPerSec={state.bytesPerSec} />
          <p className="file-card__status">
            <LockIcon size={14} />
            {t("card.transport")}
          </p>
          <CancelRow onCancel={onCancel} />
        </>
      );
    case "checking":
      return (
        <>
          <Progress card={card} received={card.size} bytesPerSec={0} />
          <p className="file-card__status">{t("card.checking")}</p>
        </>
      );
    case "done":
      return (
        <>
          <HashRow sha256={card.sha256} full />
          <p className="file-card__status">
            <span className="file-card__ok">{t("card.done")}</span>
            <span className="file-card__ok">{t("card.matches")}</span>
          </p>
          <p className="file-card__muted">{t("card.savedTo", { folder: folderOf(state.path) })}</p>
          <div className="file-card__actions">
            <button type="button" className="media-pill file-card__button" onClick={onOpenFolder}>
              <FolderIcon size={14} />
              <span>{t("card.openFolder")}</span>
            </button>
            <button type="button" className="media-pill file-card__button" onClick={onOpen}>
              <DocumentIcon size={14} />
              <span>{t("card.open")}</span>
            </button>
          </div>
        </>
      );
  }
}

/** The offer with checksum, availability, a disabled reason and Download (mockups 2, 13). */
function Offer({
  card,
  onDownload,
  retry = false,
  children,
}: {
  card: FileCard;
  onDownload: () => void;
  retry?: boolean;
  children?: ReactNode;
}) {
  const { t } = useTranslation("files");
  const ends = useEndsText(card.expires);
  const reasonId = useId();
  const block = card.downloadBlock;
  return (
    <>
      <HashRow sha256={card.sha256} />
      <p className="file-card__status">
        <span className="file-card__dot" data-online="" />
        {t("card.available", { sender: card.senderName, when: ends })}
      </p>
      {children}
      {block !== null && <BlockReason id={reasonId} block={block} />}
      <div className="file-card__actions">
        <DownloadButton
          blocked={block !== null}
          describedBy={block !== null ? reasonId : undefined}
          retry={retry}
          onClick={onDownload}
        />
      </div>
    </>
  );
}

function BlockReason({ id, block }: { id: string; block: DownloadBlock }) {
  const { t, i18n } = useTranslation("files");
  const { t: tP2p } = useTranslation("p2p");
  let text: string;
  let link: string | null = null;
  switch (block.kind) {
    case "unavailable":
      text = filesUnavailableText(tP2p, block.reason);
      break;
    case "receiveOff":
      text = t("card.disabled.receiveOff");
      link = t("card.disabled.receiveOffLink");
      break;
    case "tooLarge":
      text = t("card.disabled.tooLarge", { limit: maxSizeLabel(block.limit, i18n.language) });
      link = t("card.disabled.tooLargeLink");
      break;
    case "anotherDownload":
      text = t("card.disabled.anotherDownload");
      break;
    case "own":
      return null;
  }
  return (
    <p className="file-card__status file-card__blocked">
      <BlockedIcon size={14} />
      <span id={id}>{text}</span>
      {link !== null && (
        <button
          type="button"
          className="file-card__link"
          onClick={() => {
            openSettingsSection("privacy");
          }}
        >
          {link}
        </button>
      )}
    </p>
  );
}

function DownloadButton({
  blocked,
  describedBy,
  retry = false,
  onClick,
}: {
  blocked: boolean;
  describedBy?: string | undefined;
  retry?: boolean;
  onClick: () => void;
}) {
  const { t } = useTranslation("files");
  // aria-disabled keeps the button focusable, so keyboard and screen reader users reach the
  // reason it is disabled.
  return (
    <button
      type="button"
      className="button button--primary file-card__button"
      aria-disabled={blocked || undefined}
      aria-describedby={describedBy}
      onClick={() => {
        if (!blocked) onClick();
      }}
    >
      <DownloadIcon size={14} />
      <span>{retry ? t("card.retry") : t("card.download")}</span>
    </button>
  );
}

function CancelRow({ onCancel }: { onCancel: () => void }) {
  const { t } = useTranslation("files");
  return (
    <div className="file-card__actions">
      <button type="button" className="media-pill file-card__button" onClick={onCancel}>
        <CloseIcon size={14} />
        <span>{t("card.cancel")}</span>
      </button>
    </div>
  );
}

/** Progress bar and line (mockup 3); the bar never animates on its own. */
function Progress({
  card,
  received,
  bytesPerSec,
}: {
  card: FileCard;
  received: number;
  bytesPerSec: number;
}) {
  const { t, i18n } = useTranslation("files");
  const parts = progressParts(received, card.size, bytesPerSec, i18n.language);
  const value = t("card.progressValue", { percent: parts.percent });
  const line =
    parts.left === null
      ? value
      : t("card.progress", {
          percent: parts.percent,
          received: parts.received,
          size: parts.size,
          speed: parts.speed,
          left: parts.left,
        });
  return (
    <div className="plugin-progress plugin-progress--default">
      <div
        className="plugin-progress__bar"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={parts.percent}
        aria-valuetext={value}
        aria-label={t("card.progressLabel", { name: card.name })}
      >
        <div className="plugin-progress__fill" style={{ width: `${String(parts.percent)}%` }} />
      </div>
      <span className="plugin-progress__text">{line}</span>
    </div>
  );
}
