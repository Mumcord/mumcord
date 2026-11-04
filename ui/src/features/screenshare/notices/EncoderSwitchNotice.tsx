/**
 * "Your computer couldn't keep up with AV1 — switched to VP9" (FR-7.57, mockup 07): the card under
 * the live bar after the load fallback, with Undo while the share still offers it and Details
 * with the encode times that led to the switch. It stays while the share runs on the fallback.
 */
import { useId, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../../a11y";
import { getApi, type LoadDetails, type ShareQuality } from "../../../api";
import { FileDialog } from "../../files/dialogs/FileDialog";
import { WarningIcon } from "../quality/qualityIcons";
import { codecName } from "../quality/qualityModel";
import { useStepText } from "../quality/useStepText";
import { useAnnounceChange } from "./useAnnounceChange";
import "./notices.css";

export function EncoderSwitchNotice({
  serverId,
  quality,
}: {
  serverId: number;
  quality: ShareQuality | null;
}): ReactNode {
  const { t } = useTranslation("screenshare");
  const stepText = useStepText();
  const [details, setDetails] = useState(false);
  const titleId = useId();
  const switched = quality?.switched ?? null;
  const load = switched?.reason.kind === "load" ? switched.reason.details : null;
  const from = switched === null ? "" : codecName(switched.from.codec);
  const to = switched === null ? "" : codecName(switched.to.codec);
  const title = load === null ? null : t("notices.encoderTitle", { from, to });
  useAnnounceChange(title);
  if (quality === null || switched === null || load === null || title === null) return null;

  const undo = () => {
    getApi()
      .screenShareUndoEncoderSwitch(serverId)
      .catch((error: unknown) => {
        console.error("undoing the encoder switch failed", error);
        announce(t("notices.undoFailed", { codec: to }), "assertive");
      });
  };

  return (
    <section className="share-notice" aria-labelledby={titleId}>
      <span className="share-notice__icon">
        <WarningIcon />
      </span>
      <div className="share-notice__text">
        <p id={titleId} className="share-notice__title">
          {title}
        </p>
        <p>{t("notices.encoderBody", { quality: stepText(quality), to })}</p>
      </div>
      <span className="share-notice__actions">
        {switched.undo && (
          <button type="button" className="button" onClick={undo}>
            {t("notices.undo")}
          </button>
        )}
        <button
          type="button"
          className="button"
          aria-haspopup="dialog"
          onClick={() => {
            setDetails(true);
          }}
        >
          {t("notices.details")}
        </button>
      </span>
      {details && (
        <EncoderDetails
          from={from}
          to={to}
          load={load}
          onClose={() => {
            setDetails(false);
          }}
        />
      )}
    </section>
  );
}

/** The Details panel: why the share left its encoder, in the style of the other app dialogs. */
function EncoderDetails({
  from,
  to,
  load,
  onClose,
}: {
  from: string;
  to: string;
  load: LoadDetails;
  onClose: () => void;
}): ReactNode {
  const { t } = useTranslation("screenshare");
  const closeButton = useRef<HTMLButtonElement>(null);
  const factsId = useId();
  const ms = (value: number | null) =>
    value === null
      ? t("notices.detailsUnknown")
      : t("notices.detailsMs", { ms: Math.round(value) });
  const rows: [string, string][] = [
    [t("notices.detailsEncoder"), t("notices.detailsFromTo", { from, to })],
    [t("notices.detailsEncodeTime"), ms(load.encodeMs)],
    [t("notices.detailsBudget"), ms(load.budgetMs)],
    [t("notices.detailsDropped"), String(load.dropped)],
  ];
  return (
    <FileDialog
      title={t("notices.detailsTitle")}
      initialFocus={closeButton}
      describedBy={factsId}
      onClose={onClose}
    >
      <dl id={factsId} className="file-dialog__facts share-notice__facts">
        {rows.map(([label, value]) => (
          <div key={label} className="file-dialog__fact">
            <dt>{label}</dt>
            <dd>{value}</dd>
          </div>
        ))}
      </dl>
      <div className="app-dialog__actions">
        <button ref={closeButton} type="button" className="button" onClick={onClose}>
          {t("notices.close")}
        </button>
      </div>
    </FileDialog>
  );
}
