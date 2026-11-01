import { useState } from "react";
import { useTranslation } from "react-i18next";
import { resetSection, uniqueLabels } from "./changes";
import { ChangeListDialog } from "./components/ChangeListDialog";
import { SettingsIcon } from "./components/SettingsIcon";
import type { SectionId } from "./sectionIds";
import type { SettingsChanges } from "./useSettingsChanges";

/**
 * "Reset section" in the page header (#161): a confirm dialog lists every setting of the group
 * that changes, then puts them back to the active preset (preset-covered settings) or to their
 * default (all others). Nothing is reset without that confirmation.
 */
export function ResetSectionButton({
  section,
  changes,
}: {
  section: SectionId;
  changes: SettingsChanges;
}) {
  const { t } = useTranslation("settings");
  const [open, setOpen] = useState(false);
  const { current, baseline, apply } = changes;
  const ready = current !== null && baseline !== null;
  const plan = ready ? resetSection(current, baseline, section) : null;
  const sectionName = t(`sections.${section}`);
  return (
    <>
      <button
        type="button"
        className="settings-button settings-reset"
        disabled={!ready}
        onClick={() => {
          setOpen(true);
        }}
      >
        <SettingsIcon name="refresh" />
        {t("resetSection.button")}
      </button>
      {open && plan !== null && (
        <ChangeListDialog
          title={t("resetSection.title", { section: sectionName })}
          intro={t("resetSection.intro")}
          labels={uniqueLabels(plan.changes).map((label) => t(label))}
          emptyText={t("resetSection.nothing", { section: sectionName })}
          confirmLabel={t("resetSection.confirm")}
          onCancel={() => {
            setOpen(false);
          }}
          onConfirm={() => {
            setOpen(false);
            void apply(plan.draft);
          }}
        />
      )}
    </>
  );
}
