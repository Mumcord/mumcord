import { useId, useState, type ReactNode, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import type { MumbleImportFound } from "../../api";
import { useDate, useKeyName } from "./format";
import {
  ITEMS,
  available,
  favouriteKey,
  newFavourites,
  passwordsChosen,
  selectedCount,
  shown,
  type ImportItem,
  type ImportSelection,
} from "./selection";

interface ImportChooserProps {
  found: MumbleImportFound;
  selection: ImportSelection;
  onSelectionChange: (selection: ImportSelection) => void;
  certificatePassword: string;
  onCertificatePasswordChange: (password: string) => void;
  passwordRef: RefObject<HTMLInputElement | null>;
  /** Shown under the certificate password field. */
  passwordError: string | null;
}

function toggled<T>(set: ReadonlySet<T>, value: T, on: boolean): Set<T> {
  const next = new Set(set);
  if (on) next.add(value);
  else next.delete(value);
  return next;
}

/** The preview rows with their checkboxes (FR-3.94). */
export function ImportChooser({
  found,
  selection,
  onSelectionChange,
  certificatePassword,
  onCertificatePasswordChange,
  passwordRef,
  passwordError,
}: ImportChooserProps) {
  const { t } = useTranslation("mumbleImport");
  const keyName = useKeyName();
  const date = useDate();
  const id = useId();
  const [reviewing, setReviewing] = useState(false);
  const listId = `${id}-favourites`;
  const passwordId = `${id}-password`;
  const passwordErrorId = `${id}-password-error`;

  const setItem = (item: ImportItem, on: boolean) => {
    onSelectionChange({ ...selection, items: toggled(selection.items, item, on) });
  };

  function detail(item: ImportItem): ReactNode {
    switch (item) {
      case "favourites": {
        if (found.favouritesFailed) return t("favourites.failed");
        const duplicates = found.favourites.length - newFavourites(found).length;
        const parts = [t("favourites.found", { count: found.favourites.length })];
        if (duplicates > 0) parts.push(t("favourites.duplicates", { count: duplicates }));
        return parts.join(" · ");
      }
      case "passwords":
        return t("passwords.count", { count: passwordsChosen(found, selection) });
      case "identity": {
        const cert = found.certificate;
        if (!cert) return null;
        if (cert.alreadyImported) return t("identity.already");
        const parts = [
          cert.name === "" ? t("identity.keeps") : t("identity.named", { name: cert.name }),
        ];
        if (cert.validUntil !== null)
          parts.push(t("identity.validUntil", { date: date(cert.validUntil) }));
        if (cert.name !== "") parts.push(t("identity.keeps"));
        if (cert.needsPassword) parts.push(t("identity.locked"));
        return parts.join(" · ");
      }
      case "devices": {
        if (!available(found, "devices")) return t("devices.missing");
        const parts: string[] = [];
        if (found.inputDevice) parts.push(t("devices.input", { name: found.inputDevice.name }));
        if (found.outputDevice) parts.push(t("devices.output", { name: found.outputDevice.name }));
        return parts.join(" · ");
      }
      case "ptt":
        if (found.pttKey !== null) return keyName(found.pttKey);
        return found.pttIssue === null ? null : t(`ptt.${found.pttIssue}`);
      case "volume":
        return t("volume.hint");
    }
  }

  function aside(item: ImportItem): ReactNode {
    switch (item) {
      case "favourites":
        return available(found, "favourites") ? (
          <button
            type="button"
            className="button mumble-import__pill-button"
            aria-expanded={reviewing}
            aria-controls={listId}
            onClick={() => {
              setReviewing(!reviewing);
            }}
          >
            {reviewing ? t("favourites.hide") : t("favourites.review")}
          </button>
        ) : null;
      case "passwords":
        return <span className="mumble-import__pill">{t("passwords.keychain")}</span>;
      case "identity":
        return found.certificate?.needsPassword === true && available(found, "identity") ? (
          <span className="mumble-import__pill mumble-import__pill--warning">
            {t("identity.passwordNeeded")}
          </span>
        ) : null;
      case "volume":
        return <span className="mumble-import__aside">{t("volume.notRecommended")}</span>;
      default:
        return null;
    }
  }

  const passwordsDisabled =
    !selection.items.has("favourites") || passwordsChosen(found, selection) === 0;

  return (
    <ul className="mumble-import__list" aria-label={t("listLabel")}>
      {ITEMS.filter((item) => shown(found, item)).map((item) => {
        const usable = available(found, item) && !(item === "passwords" && passwordsDisabled);
        const checked = usable && selection.items.has(item);
        const inputId = `${id}-${item}`;
        const detailId = `${inputId}-detail`;
        return (
          <li key={item} className="mumble-import__row">
            <div className="mumble-import__main">
              <input
                id={inputId}
                type="checkbox"
                className="mumble-import__check"
                checked={checked}
                disabled={!usable}
                aria-describedby={detailId}
                onChange={(event) => {
                  setItem(item, event.target.checked);
                }}
              />
              <div className="mumble-import__text">
                <label htmlFor={inputId} className="mumble-import__title">
                  {t(`${item}.title`)}
                </label>
                <span id={detailId} className="mumble-import__detail">
                  {detail(item)}
                </span>
              </div>
              {aside(item)}
            </div>
            {item === "favourites" && reviewing && (
              <ul
                id={listId}
                className="mumble-import__favourites"
                aria-label={t("favourites.listLabel")}
              >
                {found.favourites.map((f) => {
                  const key = favouriteKey(f);
                  const favId = `${id}-fav-${key}`;
                  return (
                    <li key={key}>
                      <input
                        id={favId}
                        type="checkbox"
                        checked={!f.duplicate && selection.favourites.has(key)}
                        disabled={f.duplicate || !selection.items.has("favourites")}
                        onChange={(event) => {
                          onSelectionChange({
                            ...selection,
                            favourites: toggled(selection.favourites, key, event.target.checked),
                          });
                        }}
                      />
                      <label htmlFor={favId}>
                        {f.label}
                        <span className="mumble-import__detail">
                          {" "}
                          {f.port === 64738 ? f.host : key}
                          {f.duplicate && ` · ${t("favourites.duplicate")}`}
                        </span>
                      </label>
                    </li>
                  );
                })}
              </ul>
            )}
            {item === "identity" && checked && found.certificate?.needsPassword === true && (
              <div className="mumble-import__password">
                <label htmlFor={passwordId}>{t("identity.password")}</label>
                <input
                  ref={passwordRef}
                  id={passwordId}
                  type="password"
                  autoComplete="off"
                  value={certificatePassword}
                  aria-invalid={passwordError !== null}
                  aria-describedby={passwordError === null ? undefined : passwordErrorId}
                  onChange={(event) => {
                    onCertificatePasswordChange(event.target.value);
                  }}
                />
                {passwordError !== null && (
                  <p id={passwordErrorId} className="mumble-import__error" role="alert">
                    {passwordError}
                  </p>
                )}
              </div>
            )}
          </li>
        );
      })}
    </ul>
  );
}

/** "5 items selected", announced politely as it changes. */
export function SelectedCount({
  found,
  selection,
}: {
  found: MumbleImportFound;
  selection: ImportSelection;
}) {
  const { t } = useTranslation("mumbleImport");
  return (
    <span className="mumble-import__count" aria-live="polite">
      {t("selected", { count: selectedCount(found, selection) })}
    </span>
  );
}
