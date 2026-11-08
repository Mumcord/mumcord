import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { errorText } from "../admin/errors";
import type { AclLoad } from "./useChannelAcl";

interface PasswordTabProps {
  load: AclLoad;
  /** The password being typed; `null` while the user has not touched it. */
  value: string | null;
  onChange: (value: string) => void;
  onRetry: () => void;
  disabled: boolean;
}

/**
 * The Password tab (FR-10.28, issue #251): one password for joining the channel, written as the
 * official client does. Join rules beyond that pattern are shown as custom and left to Permissions.
 */
export function PasswordTab({ load, value, onChange, onRetry, disabled }: PasswordTabProps) {
  const { t } = useTranslation("admin");
  const id = useId();
  const [shown, setShown] = useState(false);

  if (load.status === "idle" || load.status === "loading") {
    return <p aria-busy="true">{t("channel.rulesLoading")}</p>;
  }
  if (load.status === "failed") {
    return (
      <div className="channel-window__failed" role="alert">
        <p>{errorText(t, load.error)}</p>
        <button type="button" className="button" onClick={onRetry}>
          {t("channel.retry")}
        </button>
      </div>
    );
  }
  if (load.acl.password.kind === "custom") {
    return <p className="channel-window__note">{t("channel.passwordCustom")}</p>;
  }
  const current = load.acl.password.kind === "set" ? load.acl.password.password : "";
  return (
    <div className="channel-window__field admin-field">
      <label htmlFor={id}>{t("channel.password")}</label>
      <div className="channel-window__password">
        <input
          id={id}
          type={shown ? "text" : "password"}
          value={value ?? current}
          disabled={disabled}
          autoComplete="off"
          spellCheck={false}
          aria-describedby={`${id}-hint ${id}-note`}
          onChange={(e) => {
            onChange(e.target.value);
          }}
        />
        <button
          type="button"
          className="button"
          aria-pressed={shown}
          onClick={() => {
            setShown((on) => !on);
          }}
        >
          {shown ? t("channel.passwordHide") : t("channel.passwordShow")}
        </button>
      </div>
      <p id={`${id}-hint`} className="channel-window__hint">
        {t("channel.passwordHint")}
      </p>
      <p id={`${id}-note`} className="channel-window__hint">
        {t("channel.passwordVisible")}
      </p>
    </div>
  );
}
