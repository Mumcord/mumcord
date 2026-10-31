import { useTranslation } from "react-i18next";
import { InfoTip } from "../../components/InfoTip";
import { dndSource, useDnd } from "./useDnd";
import "./dnd.css";

const MOON = "M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z";

/**
 * Shown in the user panel while Do Not Disturb is in effect, naming its source (FR-3.112). A
 * system Do Not Disturb can only be turned off on the desktop; a "?" next to it says where.
 */
export function DndIndicator() {
  const { t } = useTranslation("userpanel");
  const { t: tc } = useTranslation("connection");
  const source = dndSource(useDnd().state);
  if (!source) return null;
  return (
    <div className="dnd-indicator" data-source={source} role="status">
      <p className="dnd-indicator__badge">
        <svg className="dnd-indicator__icon" viewBox="0 0 24 24" aria-hidden="true">
          <path d={MOON} fill="currentColor" />
        </svg>
        {source === "manual" ? (
          <>
            <strong>{t("dnd.label")}</strong>
            <span className="dnd-indicator__source"> · {t("dnd.manual")}</span>
          </>
        ) : (
          <>
            <strong>{t("dnd.system")}</strong>
            <InfoTip
              label={tc("quick.moreInfo", { topic: t("dnd.system") })}
              text={t("dnd.systemHint")}
            />
          </>
        )}
      </p>
    </div>
  );
}
