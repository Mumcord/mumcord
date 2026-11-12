import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { permissionRisk } from "../permissions/catalogue";
import { usePermissionText } from "../permissions/usePermissionText";

export interface AllowAllProps {
  pluginName: string;
  /** Ids of the optional permissions the plugin asks for. */
  optional: readonly string[];
  /** Ids of the optional permissions that are switched on. */
  enabled: ReadonlySet<string>;
  disabled: boolean;
  /** Switches these optional permissions on, in addition to the ones already on. */
  onAllow: (ids: readonly string[]) => void;
}

/**
 * The intro line of the install prompt's permission list with the "Allow all" button (issue #708).
 * It switches every optional permission on at once; high-risk ones only after an extra approval.
 */
export function AllowAll({ pluginName, optional, enabled, disabled, onAllow }: AllowAllProps) {
  const { t } = useTranslation("pluginBrowse");
  const permissionText = usePermissionText();
  // The selection the approval was asked for. Every change of the selection makes a new set, so
  // switching a single permission closes the approval instead of leaving a stale list open.
  const [approvingFor, setApprovingFor] = useState<ReadonlySet<string> | null>(null);
  const titleId = useId();
  const allowAllRef = useRef<HTMLButtonElement>(null);
  const allAllowedRef = useRef<HTMLSpanElement>(null);
  const firstChoiceRef = useRef<HTMLButtonElement>(null);
  // Set when a choice in the approval closes it; switching a single permission must not move focus.
  const restoreFocus = useRef(false);

  const off = optional.filter((id) => !enabled.has(id));
  const offHighRisk = off.filter((id) => permissionRisk(id) === "red");
  const approving = approvingFor === enabled && offHighRisk.length > 0;

  // Keyboard and screen-reader users land on the approval when it opens and get back to the
  // button (or the "All allowed" note that replaced it) when it closes.
  useEffect(() => {
    if (approving) firstChoiceRef.current?.focus();
    else if (restoreFocus.current) (allowAllRef.current ?? allAllowedRef.current)?.focus();
    restoreFocus.current = false;
  }, [approving]);

  const close = () => {
    restoreFocus.current = approving;
    setApprovingFor(null);
  };
  const allow = (ids: readonly string[]) => {
    close();
    onAllow(ids);
  };

  return (
    <>
      <div className="plugin-prompt-head">
        <p className="plugin-prompt-subtitle">{t("prompt.intro", { name: pluginName })}</p>
        {off.length > 0 && (
          <button
            ref={allowAllRef}
            type="button"
            className="settings-button plugin-prompt-allow-all"
            disabled={disabled || approving}
            onClick={() => {
              if (offHighRisk.length > 0) setApprovingFor(enabled);
              else allow(off);
            }}
          >
            {t("prompt.allowAll")}
          </button>
        )}
        {optional.length > 0 && off.length === 0 && (
          <span ref={allAllowedRef} tabIndex={-1} className="plugin-prompt-all-allowed">
            <span aria-hidden="true">✓ </span>
            {t("prompt.allAllowed")}
          </span>
        )}
      </div>
      {approving && (
        <div className="plugin-prompt-approve" role="group" aria-labelledby={titleId}>
          <strong id={titleId}>{t("prompt.allowAllHighTitle")}</strong>
          <ul>
            {offHighRisk.map((id) => (
              <li key={id}>{permissionText(id)}</li>
            ))}
          </ul>
          <p>{t("prompt.allowAllHighText", { name: pluginName })}</p>
          <div className="plugin-prompt-approve-actions">
            <button
              ref={firstChoiceRef}
              type="button"
              className="settings-button settings-button--danger"
              disabled={disabled}
              onClick={() => {
                allow(off);
              }}
            >
              {t("prompt.allowAllIncludingHigh")}
            </button>
            <button
              type="button"
              className="settings-button"
              disabled={disabled}
              onClick={() => {
                allow(off.filter((id) => !offHighRisk.includes(id)));
              }}
            >
              {t("prompt.allowAllExceptHigh")}
            </button>
            <button type="button" className="settings-button" onClick={close}>
              {t("prompt.cancel")}
            </button>
          </div>
        </div>
      )}
    </>
  );
}
