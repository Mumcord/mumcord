/** Pieces shared by the received and the own file card. */
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type FileCard } from "../../../api";
import { CopyIcon, DocumentIcon, ProgramIcon } from "./fileIcons";
import { fileTypeKey, formatBytes, shortHash } from "./fileFormat";

const COPIED_MS = 2000;

/** Icon, name and "18.4 MB · PDF document" (with "· shared by you" on the own card). */
export function CardHead({ card }: { card: FileCard }) {
  const { t, i18n } = useTranslation("files");
  const vars = {
    size: formatBytes(card.size, i18n.language),
    type: t(`types.${fileTypeKey(card.name, card.mime, card.risky)}`),
  };
  return (
    <div className="audio-card__head">
      <span className="audio-card__icon" aria-hidden="true">
        {card.risky ? <ProgramIcon size={20} /> : <DocumentIcon size={20} />}
      </span>
      <div className="audio-card__text">
        <p className="audio-card__name">{card.name}</p>
        <p className="audio-card__facts">
          {card.own ? t("card.metaOwn", vars) : t("card.meta", vars)}
        </p>
      </div>
    </div>
  );
}

/**
 * The SHA-256 line: the short form with the full value as tooltip and accessible name, or the full
 * value on its own lines once the file is downloaded (mockup 4). Copy always copies the full value.
 */
export function HashRow({ sha256, full = false }: { sha256: string; full?: boolean }) {
  const { t } = useTranslation("files");
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | undefined>(undefined);

  useEffect(
    () => () => {
      window.clearTimeout(timer.current);
    },
    [],
  );

  const copy = () => {
    getApi()
      .copyText(sha256)
      .then(
        () => {
          setCopied(true);
          window.clearTimeout(timer.current);
          timer.current = window.setTimeout(() => {
            setCopied(false);
          }, COPIED_MS);
        },
        (error: unknown) => {
          console.error("Copying the checksum failed", error);
        },
      );
  };

  return (
    <div className={full ? "file-card__hash file-card__hash--full" : "file-card__hash"}>
      <span className="file-card__hash-label" aria-hidden="true">
        {t("card.checksum")}
      </span>
      <code className="file-card__hash-value" data-tooltip={full ? undefined : sha256}>
        <span aria-hidden="true">{full ? sha256 : shortHash(sha256)}</span>
        <span className="visually-hidden">{t("card.checksumLabel", { hash: sha256 })}</span>
      </code>
      <button type="button" className="media-pill file-card__copy" onClick={copy}>
        <CopyIcon size={14} />
        <span>{copied ? t("card.copied") : t("card.copy")}</span>
      </button>
    </div>
  );
}
