import { useTranslation } from "react-i18next";
import { dndSource, useDnd } from "./useDnd";
import "./dnd.css";

const MOON = "M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z";

/**
 * Compact chip in the user panel's status strip while Do Not Disturb is in effect (FR-3.112,
 * #246). The short text keeps the strip on one line; the full source ("manual", or the system
 * with where to turn it off) is the chip's accessible name and tooltip. A button like the "?"
 * info tips, so keyboard users can reach the tooltip.
 */
export function DndIndicator({ className }: { className?: string }) {
  const { t } = useTranslation("userpanel");
  const source = dndSource(useDnd().state);
  if (!source) return null;
  const label =
    source === "manual"
      ? `${t("dnd.label")} · ${t("dnd.manual")}`
      : `${t("dnd.system")}. ${t("dnd.systemHint")}`;
  return (
    <button
      type="button"
      className={className ? `dnd-indicator ${className}` : "dnd-indicator"}
      data-source={source}
      aria-label={label}
      data-tooltip={label}
    >
      <svg className="dnd-indicator__icon" viewBox="0 0 24 24" aria-hidden="true">
        <path d={MOON} fill="currentColor" />
      </svg>
      <span className="dnd-indicator__text" aria-hidden="true">
        {t("dnd.short")}
      </span>
    </button>
  );
}
