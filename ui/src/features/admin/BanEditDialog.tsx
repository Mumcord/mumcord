/**
 * The ban dialog of the ban list (issue #254, mockup `03-ban-dialog`): the same dialog for adding a
 * ban for someone who is not online and for editing an entry, with a "Who" field instead of the
 * person line. The dialog only collects the change; the window saves everything together
 * (FR-10.66), so typing here never sends anything to the server.
 */
import { useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { BanChangeInput, BanEntry } from "../../api";
import { Dialog } from "../servers/Dialog";
import { looksLikeAddress, maskAddress, shortHash } from "./banFormat";

const CHIPS: { secs: number; key: string }[] = [
  { secs: 3600, key: "chipHour" },
  { secs: 86_400, key: "chipDay" },
  { secs: 604_800, key: "chipWeek" },
  { secs: 2_592_000, key: "chipMonth" },
  { secs: 0, key: "chipPermanent" },
];

interface BanEditDialogProps {
  change: BanChangeInput;
  /** The entry the change belongs to; `null` while the user is adding a new one. */
  entry: BanEntry | null;
  onStage: (change: BanChangeInput) => void;
  onClose: () => void;
}

export function BanEditDialog({ change, entry, onStage, onClose }: BanEditDialogProps) {
  const { t, i18n } = useTranslation(["admin", "common"]);
  const isAdd = change.kind === "add";
  const [draft, setDraft] = useState({
    name: (isAdd ? change.name : entry?.name) ?? "",
    hash: (isAdd ? change.hash : entry?.certHash) ?? "",
    address: (isAdd ? change.address : entry?.address) ?? "",
    mask: isAdd ? change.mask : (entry?.mask ?? 32),
    reason: (isAdd ? change.reason : entry?.reason) ?? "",
    duration: isAdd ? change.duration : (entry?.durationSecs ?? 0),
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
    } else if (change.kind === "edit") {
      onStage({ kind: "edit", key: change.key, reason, duration: draft.duration });
    }
  };

  return (
    <Dialog
      title={isAdd ? t("admin:dialogAdd") : t("admin:dialogEdit")}
      onClose={onClose}
      className="admin-dialog"
    >
      {isAdd ? (
        <div className="admin-form" id={titleId}>
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
        </div>
      ) : (
        <p className="admin-form__help">
          {change.kind === "edit" && entry
            ? `${shortHash(entry.certHash) || "—"} · ${maskAddress(entry.address, entry.mask)}`
            : ""}
        </p>
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
        {CHIPS.map((chip) => (
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
              setDraft({ ...draft, duration: Math.max(0, Math.round((when - Date.now()) / 1000)) });
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
