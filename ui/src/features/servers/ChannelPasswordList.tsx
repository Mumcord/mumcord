import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, toEngineError } from "../../api";

interface ChannelPasswordListProps {
  serverId: number;
  /** Positions marked for removal; the dialog removes them on save. */
  removed: readonly number[];
  onRemove: (index: number) => void;
}

/**
 * The server's stored channel passwords, masked, each with a remove button (FR-2.78). The engine
 * never hands the passwords out, only how many there are.
 */
export function ChannelPasswordList({ serverId, removed, onRemove }: ChannelPasswordListProps) {
  const { t } = useTranslation("servers");
  const id = useId();
  const [count, setCount] = useState(0);

  useEffect(() => {
    let current = true;
    getApi()
      .channelPasswordCount(serverId)
      .then(
        (loaded) => {
          if (current) setCount(loaded);
        },
        (cause: unknown) => {
          console.error("Reading the channel passwords failed", toEngineError(cause).code);
        },
      );
    return () => {
      current = false;
    };
  }, [serverId]);

  const shown = Array.from({ length: count }, (_, index) => index).filter(
    (index) => !removed.includes(index),
  );
  if (shown.length === 0) return null;
  return (
    <div className="servers-form__field">
      <span id={`${id}-label`}>{t("dialog.channelPasswords")}</span>
      <ul className="servers-tokens" aria-labelledby={`${id}-label`}>
        {shown.map((index) => {
          const name = t("dialog.channelPassword", { number: index + 1 });
          return (
            <li key={index} className="servers-tokens__item">
              <span>
                {name} <span aria-hidden="true">••••••••</span>
              </span>
              <button
                type="button"
                className="button"
                aria-label={t("dialog.removeChannelPassword", { name })}
                onClick={() => {
                  onRemove(index);
                }}
              >
                {t("dialog.remove")}
              </button>
            </li>
          );
        })}
      </ul>
      <p className="servers-form__hint">{t("dialog.channelPasswordsHint")}</p>
    </div>
  );
}
