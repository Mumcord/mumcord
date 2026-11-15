import { useEffect, useId, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toEngineError, type Identity, type PhoneTransferQr } from "../../../api";
import { errorText } from "../errors";
import { ModalDialog } from "../components/ModalDialog";
import { WarningIcon } from "../components/WarningIcon";
import {
  QUIET_ZONE,
  formatLeft,
  groupCode,
  isEmpty,
  modulePx,
  qrPath,
  spokenCode,
  useSendToPhone,
} from "./sendToPhone";

/** Seconds left at which the remaining time is announced to screen readers. */
const ANNOUNCE_AT = [30, 10];

/**
 * One QR code. Always black on white, also in the dark theme: phone scanners do not read
 * inverted codes reliably. Whole pixels per module and no smoothing keep the edges sharp.
 */
function QrCode({ qr, label }: { qr: PhoneTransferQr; label: string }) {
  const modules = qr.size + 2 * QUIET_ZONE;
  const side = modules * modulePx(qr.size);
  return (
    <svg
      className="phone-transfer__qr"
      role="img"
      aria-label={label}
      width={side}
      height={side}
      viewBox={`0 0 ${String(modules)} ${String(modules)}`}
      shapeRendering="crispEdges"
      data-testid="phone-transfer-qr"
    >
      <rect width={modules} height={modules} fill="#ffffff" />
      <path d={qrPath(qr)} fill="#000000" />
    </svg>
  );
}

interface SendToPhoneDialogProps {
  identities: readonly Identity[];
  serverCount: number;
  onClose: () => void;
}

/**
 * "Send to your phone" (spec 05, FR-5.64, FR-5.67): choose identities and the server list,
 * then show them as a loop of QR codes with a code to type on the phone. Nothing secret is on
 * screen before "Show codes"; closing the dialog takes everything off again.
 */
export function SendToPhoneDialog({ identities, serverCount, onClose }: SendToPhoneDialogProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  const codesRef = useRef<HTMLDivElement>(null);
  const { selection, phase, renewed, setSelection, start } = useSendToPhone({
    identityIds: identities.filter((identity) => identity.isDefault).map((identity) => identity.id),
    includeServers: serverCount > 0,
  });
  const selecting = phase.kind === "select";
  const nothing = isEmpty(selection);

  // "Show codes" leaves the dialog when the codes appear. Focus goes to the codes, not to a
  // button, so that a stray Enter does not close the dialog while the phone is scanning.
  useEffect(() => {
    if (!selecting) codesRef.current?.focus();
  }, [selecting]);

  useEffect(() => {
    if (phase.kind === "failed") console.error("Preparing the phone transfer failed", phase.error);
  }, [phase]);

  const toggleIdentity = (identityId: number, on: boolean) => {
    const others = selection.identityIds.filter((other) => other !== identityId);
    setSelection({ ...selection, identityIds: on ? [...others, identityId] : others });
  };

  const problem =
    phase.kind !== "failed"
      ? null
      : toEngineError(phase.error).code === "invalidInput"
        ? t("identity.phoneTooLarge")
        : errorText(t, phase.error);

  return (
    <ModalDialog
      title={t("identity.phoneTitle")}
      onClose={onClose}
      className="settings-dialog--phone"
    >
      <ul className="phone-transfer__choices" aria-label={t("identity.phoneSelectionLabel")}>
        {identities.map((identity) => (
          <li key={identity.id} className="phone-transfer__choice">
            <input
              id={`${id}-identity-${String(identity.id)}`}
              type="checkbox"
              className="phone-transfer__check"
              checked={selection.identityIds.includes(identity.id)}
              onChange={(event) => {
                toggleIdentity(identity.id, event.target.checked);
              }}
            />
            <label htmlFor={`${id}-identity-${String(identity.id)}`}>
              {t(identity.isDefault ? "identity.phoneIdentityDefault" : "identity.phoneIdentity", {
                identity: identity.label,
              })}
            </label>
          </li>
        ))}
        <li className="phone-transfer__choice">
          <input
            id={`${id}-servers`}
            type="checkbox"
            className="phone-transfer__check"
            checked={selection.includeServers}
            onChange={(event) => {
              setSelection({ ...selection, includeServers: event.target.checked });
            }}
          />
          <label htmlFor={`${id}-servers`}>
            {t("identity.phoneServers", { count: serverCount })}
          </label>
        </li>
      </ul>

      {!selecting && (
        <div
          ref={codesRef}
          className="phone-transfer__codes"
          tabIndex={-1}
          role="group"
          aria-label={t("identity.phoneCodesLabel")}
        >
          {phase.kind === "preparing" && (
            <p className="settings-hint" role="status">
              {t("identity.phonePreparing")}
            </p>
          )}
          {phase.kind === "showing" && (
            <>
              <div className="phone-transfer__row">
                <QrCode
                  qr={phase.transfer.codes[phase.index] ?? { size: 0, rows: [] }}
                  label={t("identity.phoneQrLabel", {
                    current: phase.index + 1,
                    total: phase.transfer.totalParts,
                  })}
                />
                <div className="phone-transfer__progress">
                  <strong data-testid="phone-transfer-position">
                    {t("identity.phoneCodeOf", {
                      current: phase.index + 1,
                      total: phase.transfer.totalParts,
                    })}
                  </strong>
                  <div className="phone-transfer__dots" aria-hidden="true">
                    {phase.transfer.codes.map((_, index) => (
                      <i
                        // The codes have no identity of their own besides their position.
                        key={index}
                        className={index <= phase.index ? "phone-transfer__dot--on" : undefined}
                      />
                    ))}
                  </div>
                  <span className="settings-hint">
                    {phase.transfer.totalParts > 1
                      ? t("identity.phoneCycleHint", { total: phase.transfer.totalParts })
                      : t("identity.phoneSingleHint")}
                  </span>
                </div>
              </div>
              <div className="phone-transfer__row phone-transfer__row--spread">
                <div className="phone-transfer__typed">
                  <span className="settings-hint">{t("identity.phoneTypeCode")}</span>
                  <span className="phone-transfer__code" data-testid="phone-transfer-code">
                    <span aria-hidden="true">{groupCode(phase.transfer.code)}</span>
                    <span className="phone-transfer__sr-only">
                      {spokenCode(phase.transfer.code)}
                    </span>
                  </span>
                </div>
                <span className="phone-transfer__expires">
                  {t("identity.phoneExpires", { time: formatLeft(phase.secondsLeft) })}
                </span>
              </div>
            </>
          )}
          <p className="phone-transfer__sr-only" role="status">
            {phase.kind === "showing" && ANNOUNCE_AT.includes(phase.secondsLeft)
              ? t("identity.phoneExpires", { time: formatLeft(phase.secondsLeft) })
              : ""}
          </p>
          <p className="phone-transfer__renewed" role="status">
            {renewed && phase.kind === "showing" ? t("identity.phoneRenewed") : ""}
          </p>
        </div>
      )}

      <p className="identity-warning identity-warning--caution">
        <WarningIcon className="identity-warning__icon" />
        {t("identity.phoneWarning")}
      </p>
      {selecting && nothing && (
        <p className="settings-hint">{t("identity.phoneNothingSelected")}</p>
      )}
      <div role="alert" className="settings-error">
        {problem}
      </div>
      <div className="settings-dialog__actions">
        <button type="button" className="settings-button" onClick={onClose}>
          {t("cancel", { ns: "common" })}
        </button>
        {selecting ? (
          <button
            type="button"
            className="settings-button settings-button--primary"
            disabled={nothing}
            aria-disabled={nothing}
            onClick={start}
          >
            {t("identity.phoneShow")}
          </button>
        ) : (
          <button
            type="button"
            className="settings-button settings-button--primary"
            onClick={onClose}
          >
            {t("identity.phoneDone")}
          </button>
        )}
      </div>
    </ModalDialog>
  );
}
