/**
 * Ban dialog (issue #253, FR-10.12, mockup `03-ban-dialog`): reason, duration chips with the end in
 * local time, "Ban by" certificate and IP address with its mask, "Remove from server now", and the
 * buttons "Kick only instead", "Cancel" and "Ban". A server whose list cannot be read only allows its
 * own default length (FR-10.13), a target the server cannot match stays off with its reason
 * (FR-10.14), and the address is masked by default because it is personal data shown only to admins
 * (FR-10.65). Enter confirms, Escape cancels (FR-10.56), and a refusal stays inside the open dialog
 * (FR-10.59). A conflict is asked about before anything is overwritten (FR-10.66).
 */
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { actionErrorText } from "./errors";
import { DURATION_CHIPS, maskAddress, plain, shortHash } from "./banFormat";
import { PersonLine } from "./PersonLine";
import {
  banInput,
  closePersonAction,
  loadTarget,
  saveAnyway,
  sendBan,
  setDraft,
  switchMode,
  type PersonAction,
} from "./personActionStore";
import { Dialog } from "../servers/Dialog";
import { initials } from "../servers/format";

/**
 * A switch row as the mockup draws it: the switch first, then the label and what it applies to.
 * The row itself carries the state, so it stays reachable and readable with a screen reader.
 */
function SwitchRow({
  label,
  checked,
  onChange,
  hint,
  disabled,
}: {
  label: string;
  checked: boolean;
  // A row that is fixed on has nothing to change (FR-10.14).
  onChange?: (checked: boolean) => void;
  hint?: string;
  disabled: boolean;
}) {
  return (
    <div className="admin-switch">
      <input
        type="checkbox"
        role="switch"
        className="admin-switch__input"
        checked={checked}
        aria-checked={checked}
        disabled={disabled}
        aria-label={label}
        onChange={(event) => {
          onChange?.(event.target.checked);
        }}
      />
      <span className="admin-switch__text">
        <span className="admin-switch__label">{label}</span>
        {hint ? <span className="admin-form__help">{hint}</span> : null}
      </span>
    </div>
  );
}

export function BanDialog({ action, serverName }: { action: PersonAction; serverName: string }) {
  const { t, i18n } = useTranslation(["admin", "common"]);
  const id = useId();
  const [custom, setCustom] = useState(false);
  const [revealed, setRevealed] = useState(false);
  // The end time is measured once, so the render stays pure.
  const [now] = useState(() => Date.now());

  const target = action.target;
  // Mumcord can only write an entry when it knows the address, so without one the server's own ban
  // is what works (issue #254, spike S1).
  const quick = target !== null && (!target.banListReadable || target.address === null);
  const noCert = target !== null && target.certHash === null;
  const family = target !== null && target.address?.includes(":") ? 128 : 32;
  // A smaller number is a wider range: /24 covers more addresses than /32 (issue #253).
  const wide = action.draft.mask < family;
  const end =
    quick || action.draft.duration === 0 ? null : new Date(now + action.draft.duration * 1000);
  const byCert = quick ? true : action.draft.byCert;
  const byIp = quick ? true : action.draft.byIp;

  const durationLabel = custom
    ? t("chipCustom")
    : (DURATION_CHIPS.find((chip) => chip.secs === action.draft.duration)?.key ?? "chipPermanent");
  const submitLabel = quick
    ? t("ban")
    : action.draft.duration === 0
      ? t("banPermanently")
      : t("banFor", { duration: t(durationLabel) });

  const submit = () => {
    void sendBan(banInput(action));
  };

  return (
    <Dialog
      title={`${t("banTitle", { name: action.name })} · ${serverName}`}
      titleLead={
        <span className="admin-title-mark" aria-hidden="true">
          {initials(serverName)}
        </span>
      }
      onClose={() => {
        closePersonAction();
      }}
      className="admin-dialog"
    >
      <PersonLine action={action} />

      {action.loading ? (
        <p className="admin-notice" role="status">
          {t("common:loading")}
        </p>
      ) : null}

      <form
        className="admin-form"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <div className="admin-form__field">
          <label htmlFor={`${id}-reason`}>{t("reason")}</label>
          <input
            id={`${id}-reason`}
            type="text"
            value={action.draft.reason}
            onChange={(event) => {
              setDraft({ reason: event.target.value });
            }}
          />
          <span className="admin-form__help">
            {t("reasonShownAndSaved", { name: action.name })}
          </span>
        </div>

        <div className="admin-chips" role="group" aria-label={t("duration")}>
          {quick ? (
            <button type="button" className="button button--small" aria-pressed="true" disabled>
              {t("serverDefault")}
            </button>
          ) : (
            DURATION_CHIPS.map((chip) => (
              <button
                key={chip.key}
                type="button"
                className="button button--small"
                aria-pressed={!custom && action.draft.duration === chip.secs}
                disabled={action.busy}
                onClick={() => {
                  setCustom(false);
                  setDraft({ duration: chip.secs });
                }}
              >
                {t(chip.key)}
              </button>
            ))
          )}
          <button
            type="button"
            className="button button--small"
            aria-pressed={custom}
            disabled={quick || action.busy}
            onClick={() => {
              setCustom(true);
            }}
          >
            {t("chipCustom")}
          </button>
        </div>

        {custom ? (
          <div className="admin-form__field">
            <label htmlFor={`${id}-date`}>{t("endsOnDate")}</label>
            <input
              id={`${id}-date`}
              type="date"
              onChange={(event) => {
                const when = event.target.valueAsNumber;
                if (Number.isNaN(when)) return;
                setDraft({ duration: Math.max(0, Math.round((when - Date.now()) / 1000)) });
              }}
            />
          </div>
        ) : null}

        <p className="admin-ends">
          {end === null
            ? quick
              ? t("quickNotice")
              : t("endsPermanent")
            : t("endsOn", {
                date: new Intl.DateTimeFormat(i18n.language, { dateStyle: "long" }).format(end),
              })}
        </p>

        <div className="admin-form" role="group" aria-label={t("banBy")}>
          <SwitchRow
            label={t("certificate")}
            checked={byCert}
            disabled={quick || noCert || action.busy}
            onChange={(checked) => {
              setDraft({ byCert: checked });
            }}
            hint={
              quick
                ? t("quickFixed")
                : noCert
                  ? t("noCertificate")
                  : `${shortHash(target?.certHash ?? null)} – ${t("certRegisteredHint")}`
            }
          />
          {/* The address is what a standard server matches, so this tick stays on (FR-10.14). */}
          <SwitchRow
            label={t("ipAddress")}
            checked
            disabled
            hint={
              quick
                ? t("quickFixed")
                : `${revealed ? plain(target?.address ?? "") : maskAddress(target?.address ?? "", action.draft.mask)}/${String(action.draft.mask)} ${t(maskHintKey(action.draft.mask, family))} – ${t("ipAlwaysOnHint")}`
            }
          />
          {!quick && byIp ? (
            <div className="admin-form__field">
              <label htmlFor={`${id}-mask`}>{t("mask")}</label>
              <div className="admin-mask">
                <span className="admin-mask__sign" aria-hidden="true">
                  /
                </span>
                <input
                  id={`${id}-mask`}
                  type="number"
                  min="0"
                  max={family}
                  value={action.draft.mask}
                  onChange={(event) => {
                    setDraft({ mask: event.target.valueAsNumber || 0 });
                  }}
                />
              </div>
              <button
                type="button"
                className="button button--small"
                aria-pressed={revealed}
                onClick={() => {
                  setRevealed(!revealed);
                }}
              >
                {revealed ? t("hide") : t("show")}
              </button>
            </div>
          ) : null}
          {wide && !quick ? (
            <p className="admin-notice admin-notice--danger">{t("maskWarn")}</p>
          ) : null}
        </div>

        <SwitchRow
          label={t("removeNow")}
          checked={action.draft.removeNow}
          disabled={action.busy}
          onChange={(checked) => {
            setDraft({ removeNow: checked });
          }}
          hint={t("removeNowNote")}
        />

        {action.conflict ? (
          <div className="admin-notice">
            <p>{t("conflictTitle")}</p>
            <p>{t("conflictLine")}</p>
            <div className="admin-footer__buttons">
              <button type="button" className="button" onClick={() => void loadTarget()}>
                {t("conflictReload")}
              </button>
              <button
                type="button"
                className="button"
                onClick={() => {
                  closePersonAction();
                }}
              >
                {t("common:back")}
              </button>
              <button
                type="button"
                className="button"
                onClick={() => {
                  saveAnyway(banInput(action));
                }}
              >
                {t("conflictSaveAnyway")}
              </button>
            </div>
          </div>
        ) : null}

        {action.error ? (
          <p className="admin-error" role="alert">
            {actionErrorText(t, action.error)}
          </p>
        ) : null}

        <div className="servers-dialog__actions">
          <button
            type="button"
            className="button"
            disabled={action.busy || (action.target !== null && !action.target.canKick)}
            onClick={() => {
              switchMode("kick");
            }}
          >
            {t("kickOnlyInstead")}
          </button>
          <button
            type="button"
            className="button"
            onClick={() => {
              closePersonAction();
            }}
          >
            {t("common:cancel")}
          </button>
          <button
            type="submit"
            className="button servers-button--danger"
            disabled={action.busy || (!byCert && !byIp)}
          >
            {submitLabel}
          </button>
        </div>
      </form>
    </Dialog>
  );
}

/** "/32 this address only", "/24 a wider range": the mask's meaning, so it is not left to guess. */
function maskHintKey(mask: number, family: number): string {
  return mask === family ? "maskExact" : "maskRange";
}
