import { useTranslation } from "react-i18next";
import { useHintPreferences } from "./hintPreferences";
import "./skip-links.css";

export interface SkipTarget {
  /** Element id; the target needs `tabIndex={-1}` so it can receive focus. */
  id: string;
  label: string;
}

/**
 * Links that become visible on keyboard focus and jump past the navigation columns. Not rendered
 * while the user has turned skip links off.
 */
export function SkipLinks({ targets }: { targets: SkipTarget[] }) {
  const { t } = useTranslation("app");
  const { skipLinks } = useHintPreferences();
  if (!skipLinks) return null;
  return (
    <nav className="skip-links" aria-label={t("skipLinks.label")}>
      {targets.map((target) => (
        <a key={target.id} className="skip-links__link" href={`#${target.id}`}>
          {target.label}
        </a>
      ))}
    </nav>
  );
}
