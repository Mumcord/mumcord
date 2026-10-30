import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, toEngineError, type SavedChannelPassword } from "../../api";
import { channelPasswordName } from "./channelPasswordName";

interface ChannelPasswordListProps {
  serverId: number;
  /** Positions marked for removal; the dialog removes them on save. */
  removed: readonly number[];
  onRemove: (index: number) => void;
  /** New names by position; the dialog saves them on save. An empty one means the default. */
  renamed: ReadonlyMap<number, string>;
  onRename: (index: number, label: string) => void;
}

/**
 * The server's stored channel passwords, masked, each with the channels it was entered for,
 * its last use, a rename and a remove button (FR-2.78, issue #146). The engine never hands
 * the passwords out.
 */
export function ChannelPasswordList({
  serverId,
  removed,
  onRemove,
  renamed,
  onRename,
}: ChannelPasswordListProps) {
  const { t, i18n } = useTranslation("servers");
  const id = useId();
  const [saved, setSaved] = useState<SavedChannelPassword[]>([]);
  const [editing, setEditing] = useState<number | null>(null);
  const nameField = useRef<HTMLInputElement>(null);

  // The name field replaces the Rename button's row text, so focus moves into it.
  useEffect(() => {
    if (editing !== null) nameField.current?.focus();
  }, [editing]);

  useEffect(() => {
    let current = true;
    getApi()
      .channelPasswords(serverId)
      .then(
        (loaded) => {
          if (current) setSaved(loaded);
        },
        (cause: unknown) => {
          console.error("Reading the channel passwords failed", toEngineError(cause).code);
        },
      );
    return () => {
      current = false;
    };
  }, [serverId]);

  const shown = saved
    .map((password, index) => ({ password, index }))
    .filter(({ index }) => !removed.includes(index));
  if (shown.length === 0) return null;
  const lastUsed = (at: string) =>
    t("dialog.channelPasswordLastUsed", {
      when: new Intl.DateTimeFormat(i18n.language, {
        dateStyle: "medium",
        timeStyle: "short",
      }).format(new Date(at)),
    });
  return (
    <div className="servers-form__field">
      <span id={`${id}-label`}>{t("dialog.channelPasswords")}</span>
      <ul className="servers-tokens" aria-labelledby={`${id}-label`}>
        {shown.map(({ password, index }) => {
          const label = renamed.get(index) ?? password.label ?? "";
          const name = channelPasswordName(t, password, index, label);
          const fallback = channelPasswordName(t, password, index, "");
          const isEditing = editing === index;
          return (
            <li key={index} className="servers-tokens__item">
              <div className="servers-tokens__text">
                {isEditing ? (
                  <input
                    className="input"
                    aria-label={t("dialog.channelPasswordName", { name: fallback })}
                    placeholder={fallback}
                    value={label}
                    ref={nameField}
                    onChange={(event) => {
                      onRename(index, event.target.value);
                    }}
                    onKeyDown={(event) => {
                      // Enter would save the whole dialog and Escape close it: here both
                      // only end the rename.
                      if (event.key === "Enter" || event.key === "Escape") {
                        event.preventDefault();
                        event.stopPropagation();
                        setEditing(null);
                      }
                    }}
                  />
                ) : (
                  <span>
                    {name} <span aria-hidden="true">••••••••</span>
                  </span>
                )}
                {password.channels.map((channel) => (
                  <span key={channel.channelId} className="servers-form__hint">
                    {channel.path.join(" › ")}
                    {channel.found === false && ` ${t("dialog.channelPasswordNotFound")}`}
                  </span>
                ))}
                {password.lastUsedAt !== null && (
                  <span className="servers-form__hint">{lastUsed(password.lastUsedAt)}</span>
                )}
              </div>
              <div className="servers-tokens__actions">
                <button
                  type="button"
                  className="button"
                  aria-label={isEditing ? undefined : t("dialog.renameChannelPassword", { name })}
                  onClick={() => {
                    setEditing(isEditing ? null : index);
                  }}
                >
                  {isEditing ? t("dialog.renameDone") : t("dialog.rename")}
                </button>
                <button
                  type="button"
                  className="button"
                  aria-label={t("dialog.removeChannelPassword", { name })}
                  onClick={() => {
                    if (isEditing) setEditing(null);
                    onRemove(index);
                  }}
                >
                  {t("dialog.remove")}
                </button>
              </div>
            </li>
          );
        })}
      </ul>
      <p className="servers-form__hint">{t("dialog.channelPasswordsHint")}</p>
    </div>
  );
}
