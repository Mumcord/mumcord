/**
 * The one form for adding and changing a ban (issue #254, mockup `03-ban-dialog`; one shape for
 * both jobs, issue #436). Adding asks "Who", editing starts from the selected entry; reason and
 * end work the same in both. The dialog only collects the change; the window saves everything
 * together (FR-10.66), so typing here never sends anything to the server.
 */
import { useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { BanChangeInput, BanEntry } from "../../api";
import { Dialog } from "../servers/Dialog";
import { initials } from "../servers/format";
import { DURATION_CHIPS, looksLikeAddress, maskAddress } from "./banFormat";

/** The two kinds this dialog writes; a removal has no form. */
export type BanDraft =
  Extract<BanChangeInput, { kind: "add" }> | Extract<BanChangeInput, { kind: "edit" }>;

interface BanEditDialogProps {
  change: BanDraft;
  /** The entry the change belongs to; `null` while the user is adding a new one. */
  entry: BanEntry | null;
  serverName: string;
  onStage: (change: BanChangeInput) => void;
  onClose: () => void;
}

export function BanEditDialog({ change, entry, serverName, onStage, onClose }: BanEditDialogProps) {
  const { t, i18n } = useTranslation(["admin", "common"]);
  const isAdd = change.kind === "add";
  const [draft, setDraft] = useState({
    name: (isAdd ? change.name : entry?.name) ?? "",
    hash: (isAdd ? change.hash : entry?.certHash) ?? "",
    address: (isAdd ? change.address : entry?.address) ?? "",
    mask: isAdd ? change.mask : (entry?.mask ?? 32),
    reason: change.reason ?? "",
    duration: change.duration,
  });
  const [custom, setCustom] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The preview is measured once, so the render stays pure.
  const [now] = useState(() => Date.now());
  const titleId = useId();

  const end = useMemo(
    () => (draft.duration === 0 ? null : new Date(now + draft.duration * 1000)),
    [draft.duration, now],
  );

  const submit = () => {
    if (isAdd && !looksLikeAddress(draft.address)) {
      setError(t("admin:errors.invalidAddress"));
      return;
    }
    const reason = draft.reason.trim() === "" ? null : draft.reason.trim();
    if (change.kind === "add") {
      onStage({
        kind: "add",
        name: draft.name.trim() === "" ? null : draft.name.trim(),
        hash: draft.hash.trim() === "" ? null : draft.hash.trim(),
        address: draft.address,
        mask: draft.mask,
        reason,
        duration: draft.duration,
      });
    } else {
      onStage({ kind: "edit", key: change.key, reason, duration: draft.duration });
    }
  };

  const name = draft.name.trim() === "" ? t("admin:noName") : draft.name.trim();

  return (
    <Dialog
      title={`${isAdd ? t("admin:dialogAdd") : t("admin:dialogEdit")} · ${serverName}`}
      titleLead={
        <span className="admin-title-mark" aria-hidden="true">
          {initials(serverName)}
        </span>
      }
      onClose={onClose}
      className="admin-dialog"
    >
      <div className="admin-form">
        {isAdd ? (
          <>
            <div className="admin-form__field">
              <label htmlFor={`${titleId}-name`}>{t("admin:who")}</label>
              <input
                id={`${titleId}-name`}
                type="text"
                value={draft.name}
                onChange={(event) => {
                  setDraft({ ...draft, name: event.target.value });
                }}
              />
            </div>
            <div className="admin-form__field">
              <label htmlFor={`${titleId}-hash`}>{t("admin:fingerprint")}</label>
              <input
                id={`${titleId}-hash`}
                type="text"
                value={draft.hash}
                onChange={(event) => {
                  setDraft({ ...draft, hash: event.target.value });
                }}
              />
            </div>
            <div className="admin-form__field">
              <label htmlFor={`${titleId}-address`}>{t("admin:ipAddress")}</label>
              <input
                id={`${titleId}-address`}
                type="text"
                value={draft.address}
                onChange={(event) => {
                  setDraft({ ...draft, address: event.target.value });
                }}
                placeholder="203.0.113.1"
              />
              <input
                id={`${titleId}-mask`}
                type="number"
                min="0"
                max="128"
                value={draft.mask}
                onChange={(event) => {
                  setDraft({ ...draft, mask: event.target.valueAsNumber || 0 });
                }}
                aria-label={t("admin:mask")}
              />
              <span className="admin-form__help">{t("admin:maskHelp")}</span>
              {draft.address !== "" ? (
                <span className="admin-form__help">{maskAddress(draft.address, draft.mask)}</span>
              ) : null}
            </div>
          </>
        ) : (
          <dl className="admin-dialog__entry">
            <dt>{t("admin:name")}</dt>
            <dd>{name}</dd>
            <dt>{t("admin:fingerprint")}</dt>
            <dd className="admin-dialog__value">{draft.hash || "—"}</dd>
            <dt>{t("admin:ipAddress")}</dt>
            <dd className="admin-dialog__value">
              {draft.address === "" ? "—" : `${draft.address}/${String(draft.mask)}`}
            </dd>
            {entry?.expired ? (
              <>
                <dt>{t("admin:columns.ends")}</dt>
                <dd>{t("admin:endsExpired")}</dd>
              </>
            ) : null}
          </dl>
        )}

        <div className="admin-form__field">
          <label htmlFor={`${titleId}-reason`}>{t("admin:reason")}</label>
          <input
            id={`${titleId}-reason`}
            type="text"
            value={draft.reason}
            onChange={(event) => {
              setDraft({ ...draft, reason: event.target.value });
            }}
          />
        </div>

        <div className="admin-chips" role="group" aria-label={t("admin:duration")}>
          {DURATION_CHIPS.map((chip) => (
            <button
              key={chip.key}
              type="button"
              className="button button--small"
              aria-pressed={!custom && draft.duration === chip.secs}
              onClick={() => {
                setCustom(false);
                setDraft({ ...draft, duration: chip.secs });
              }}
            >
              {t(`admin:${chip.key}`)}
            </button>
          ))}

          <button
            type="button"
            className="button button--small"
            aria-pressed={custom}
            onClick={() => {
              setCustom(true);
            }}
          >
            {t("admin:chipCustom")}
          </button>
        </div>

        {custom ? (
          <div className="admin-form__field">
            <label htmlFor={`${titleId}-date`}>{t("admin:endsOnDate")}</label>
            <input
              id={`${titleId}-date`}
              type="date"
              onChange={(event) => {
                const when = event.target.valueAsNumber;
                if (Number.isNaN(when)) return;
                setDraft({
                  ...draft,
                  duration: Math.max(0, Math.round((when - Date.now()) / 1000)),
                });
              }}
            />
          </div>
        ) : null}

        <p className="admin-ends">
          {end === null
            ? t("admin:endsPermanent")
            : t("admin:endsOn", {
                date: new Intl.DateTimeFormat(i18n.language, { dateStyle: "long" }).format(end),
              })}
        </p>

        {error ? (
          <p className="admin-error" role="alert">
            {error}
          </p>
        ) : null}
      </div>

      <div className="servers-dialog__actions">
        <button type="button" className="button" onClick={onClose}>
          {t("common:cancel")}
        </button>
        <button type="button" className="button button--primary" onClick={submit}>
          {isAdd ? t("admin:stageAdd") : t("admin:stageChange")}
        </button>
      </div>
    </Dialog>
  );
}
