import { useId, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { InfoTip } from "../../../components/InfoTip";
import { SettingsIcon, type SettingsIconName } from "./SettingsIcon";

/**
 * Section card inside a settings page: icon, title and the rows. A labelled region, so screen
 * readers can jump between cards. `data-title` lets a search jump find the card by its title.
 */
export function Group({
  title,
  icon,
  badge,
  info,
  actions,
  wide = false,
  children,
}: {
  title: string;
  icon?: SettingsIconName;
  badge?: ReactNode;
  /** Explains the whole card behind a "?" next to the title instead of a paragraph. */
  info?: string;
  /** Buttons in the card header, e.g. "New identity". */
  actions?: ReactNode;
  /** Takes the full page width instead of one column, for cards with lists or editors. */
  wide?: boolean;
  children: ReactNode;
}) {
  const { t } = useTranslation("settings");
  const titleId = useId();
  return (
    <section
      className={wide ? "settings-group settings-group--wide" : "settings-group"}
      aria-labelledby={titleId}
      data-title={title}
    >
      <div className="settings-group__head">
        {icon !== undefined && (
          <span className="settings-group__icon">
            <SettingsIcon name={icon} />
          </span>
        )}
        <h3 id={titleId} className="settings-group__title">
          {title}
          {badge}
        </h3>
        {info !== undefined && <InfoTip label={t("moreInfo", { topic: title })} text={info} />}
        {actions !== undefined && <div className="settings-group__actions">{actions}</div>}
      </div>
      {children}
    </section>
  );
}
