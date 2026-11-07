/** The paperclip next to the message input: opens the file picker, or says why it cannot. */
import { useTranslation } from "react-i18next";
import { ComposerIcon } from "../../../components/ComposerIcon";
import { pickFiles } from "./pickFiles";

const PAPERCLIP =
  "M21.4 11.1l-9.2 9.2a6 6 0 0 1-8.5-8.5l9.2-9.2a4 4 0 0 1 5.7 5.7l-9.2 9.2a2 2 0 0 1-2.8-2.8l8.5-8.5";

export function PaperclipButton({
  disabled,
  reason,
  describedBy,
  onPick,
}: {
  disabled: boolean;
  /** Why sharing is unavailable; shown as the tooltip and description of the disabled button. */
  reason: string | null;
  /** Id of the visible line that states the reason. */
  describedBy?: string | undefined;
  onPick: (paths: string[]) => void;
}) {
  const { t } = useTranslation("files");
  const label = reason ?? t("composer.attach");
  return (
    <button
      type="button"
      className="chat-composer__icon"
      aria-label={t("composer.attachLabel")}
      data-tooltip={label}
      aria-disabled={disabled || reason !== null ? true : undefined}
      aria-describedby={reason !== null ? describedBy : undefined}
      disabled={disabled}
      onClick={() => {
        if (reason !== null) return;
        pickFiles(t("composer.attachLabel")).then(onPick, (err: unknown) => {
          console.error("the file picker failed", err);
        });
      }}
    >
      <ComposerIcon d={PAPERCLIP} />
    </button>
  );
}
