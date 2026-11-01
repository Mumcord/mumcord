import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { SetupPreset } from "../../api";
import { PresetCards } from "../wizard/PresetCards";
import { presetDifferences, type PresetDraft, type PresetId } from "../wizard/presets";
import { activePresetOf, presetSwitchPlan } from "./changes";
import { ChangeList } from "./components/ChangeListDialog";
import { ModalDialog } from "./components/ModalDialog";
import type { SettingsChanges } from "./useSettingsChanges";

/** The plain consequences each preset card lists (3–4). */
const POINT_KEYS: Record<SetupPreset, readonly string[]> = {
  everythingOn: ["point1", "point2", "point3", "point4"],
  balanced: ["point1", "point2", "point3", "point4"],
  maximumPrivacy: ["point1", "point2", "point3", "point4"],
  custom: ["point1", "point2", "point3"],
};

/** The name the card shows: the preset, "Balanced, customised", "Custom" or "No preset". */
function usePresetName(changes: SettingsChanges): string {
  const { t } = useTranslation("settings");
  const setupPreset = changes.current?.settings.setupPreset ?? null;
  if (setupPreset === null) return t("activePreset.none");
  const name = t(`presets.${setupPreset}.name`);
  const preset: PresetId | null = activePresetOf(setupPreset);
  if (preset === null || changes.current === null) return name;
  return presetDifferences(changes.current, preset).length > 0
    ? t("activePreset.customised", { preset: name })
    : name;
}

/**
 * Sidebar footer (#161): the active setup preset (#159), how many settings differ from it, and
 * "Switch", which opens the wizard's preset cards and applies the chosen one after a confirm
 * that lists every setting that changes.
 */
export function ActivePresetCard({ changes }: { changes: SettingsChanges }) {
  const { t } = useTranslation("settings");
  const name = usePresetName(changes);
  const [open, setOpen] = useState(false);
  const count = changes.changed.length;
  return (
    <section className="active-preset" aria-label={t("activePreset.heading")}>
      <p className="active-preset__heading" aria-hidden="true">
        {t("activePreset.heading")}
      </p>
      <div className="active-preset__row">
        <p className="active-preset__name">{name}</p>
        <button
          type="button"
          className="active-preset__switch"
          aria-label={t("activePreset.switchLabel")}
          disabled={changes.current === null}
          onClick={() => {
            setOpen(true);
          }}
        >
          {t("activePreset.switch")}
        </button>
      </div>
      <p className="active-preset__count">
        {count === 0 ? t("activePreset.unchanged") : t("activePreset.changed", { count })}
      </p>
      {open && changes.current !== null && (
        <PresetSwitchDialog
          current={changes.current}
          onApply={(draft) => {
            setOpen(false);
            void changes.apply(draft);
          }}
          onClose={() => {
            setOpen(false);
          }}
        />
      )}
    </section>
  );
}

/** Pick a preset, then confirm the list of settings it changes. */
function PresetSwitchDialog({
  current,
  onApply,
  onClose,
}: {
  current: PresetDraft;
  onApply: (draft: PresetDraft) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation("settings");
  const { t: tc } = useTranslation("common");
  const [chosen, setChosen] = useState<SetupPreset | null>(current.settings.setupPreset);
  const [confirming, setConfirming] = useState(false);

  const plan = chosen === null ? null : presetSwitchPlan(current, chosen);
  const name = chosen === null ? "" : t(`presets.${chosen}.name`);
  // One dialog for both steps, so focus stays inside and returns to "Switch" afterwards.
  return (
    <ModalDialog
      title={confirming ? t("presets.confirmTitle", { preset: name }) : t("presets.dialogTitle")}
      onClose={onClose}
      className="preset-dialog"
    >
      {confirming && plan !== null ? (
        <ChangeList
          intro={
            chosen === "custom"
              ? t("presets.confirmCustom")
              : plan.labels.length === 0
                ? t("presets.confirmNothing", { preset: name })
                : t("presets.confirmIntro")
          }
          labels={plan.labels.map((label) => t(label))}
        />
      ) : (
        <>
          <p>{t("presets.dialogIntro")}</p>
          <PresetCards
            value={chosen}
            onChange={setChosen}
            legend={t("presets.legend")}
            text={(preset) => ({
              name: t(`presets.${preset}.name`),
              tagline: t(`presets.${preset}.tagline`),
              points: POINT_KEYS[preset].map((point) => t(`presets.${preset}.${point}`)),
            })}
          />
        </>
      )}
      <div className="settings-dialog__actions">
        <button
          type="button"
          className="settings-button"
          onClick={() => {
            if (confirming) setConfirming(false);
            else onClose();
          }}
        >
          {confirming ? tc("back") : tc("cancel")}
        </button>
        <button
          type="button"
          className="settings-button settings-button--primary"
          disabled={plan === null}
          onClick={() => {
            if (!confirming) setConfirming(true);
            else if (plan !== null) onApply(plan.draft);
          }}
        >
          {confirming ? t("presets.apply") : t("presets.continue")}
        </button>
      </div>
    </ModalDialog>
  );
}
