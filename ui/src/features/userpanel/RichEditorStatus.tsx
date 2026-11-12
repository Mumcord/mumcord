import { useTranslation } from "react-i18next";
import { ComposerIcon } from "../../components/ComposerIcon";
import type { LengthMeasure } from "./richEditorModel";
import { EDITOR_ICONS } from "./richEditorIcons";

export interface EditorNotice {
  kind: "info" | "error";
  text: string;
}

interface RichEditorStatusProps {
  /** Shown when nothing more pressing is: what the text is for, or how Source works. */
  hint: string;
  notice: EditorNotice | null;
  measure: LengthMeasure;
}

/** The line under the text: a hint or problem on the left, the length meter on the right. */
export function RichEditorStatus({ hint, notice, measure }: RichEditorStatusProps) {
  const { t, i18n } = useTranslation("userpanel");
  const number = (value: number, digits = 0) =>
    value.toLocaleString(i18n.language, {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    });
  const kb = (chars: number) => number(chars / 1024, 1);
  const meterText = measure.images
    ? t("editor.meterKb", { used: kb(measure.used), limit: number(measure.limit / 1024) })
    : t("editor.meterText", { used: number(measure.used), limit: number(measure.limit) });
  const over = measure.level === "over";
  const overText = measure.images
    ? t("editor.overImage", {
        size: t("editor.sizeKb", { size: kb(measure.used - measure.limit) }),
      })
    : t("editor.overText", { used: number(measure.used), limit: number(measure.limit) });
  const message: EditorNotice =
    notice ?? (over ? { kind: "error", text: overText } : { kind: "info", text: hint });
  const error = message.kind === "error";

  return (
    <div className="rte__status">
      <span
        className={`rte__message${error ? " rte__message--error" : ""}`}
        role={error ? "alert" : "status"}
      >
        {error && <ComposerIcon d={EDITOR_ICONS.warn} size={14} />}
        {message.text}
      </span>
      <span
        className="rte__meter"
        role="meter"
        aria-label={measure.images ? t("editor.meterImages") : t("editor.meterLength")}
        aria-valuemin={0}
        aria-valuemax={measure.limit}
        aria-valuenow={Math.min(measure.used, measure.limit)}
        aria-valuetext={meterText}
        data-tooltip={measure.images ? t("editor.meterImageTip") : t("editor.meterTextTip")}
      >
        <span className="rte__meter-track" aria-hidden="true">
          <span
            className="rte__meter-fill"
            style={{ width: `${String(Math.min(1, measure.used / measure.limit) * 100)}%` }}
          />
        </span>
        <span className="rte__meter-text" aria-hidden="true">
          {meterText}
        </span>
      </span>
    </div>
  );
}
