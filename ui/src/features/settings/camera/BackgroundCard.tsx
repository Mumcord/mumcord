import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type CameraBackground, type CameraEffectQuality } from "../../../api";
import { Group } from "../components/Section";
import { InfoLineIcon } from "./InfoLineIcon";
import { SelectRow } from "../sections/ScreenSharingSection";
import type { SettingsSectionProps } from "../sections/types";
import { backgroundOf, choiceOf, type BackgroundChoice } from "./backgroundChoice";
import { BlurStrengthButtons } from "./BlurStrengthButtons";
import { ManageImagesDialog } from "./ManageImagesDialog";
import { ownImageNumber, useOwnImages } from "./useOwnImages";

const EFFECT_QUALITIES: readonly CameraEffectQuality[] = ["auto", "best", "light"];
const EFFECT_KEYS = {
  auto: "camera.effectAuto",
  best: "camera.effectBest",
  light: "camera.effectLight",
} as const;

/** Applies a saved background to the running Settings preview (FR-8.76). */
function applyToPreview(background: CameraBackground, blurStrength: number) {
  getApi()
    .cameraSetBackground(background, blurStrength)
    .catch((error: unknown) => {
      console.error("Changing the preview background failed", error);
    });
}

/**
 * Background card of Settings → Camera (mockup 11): default background, blur strength, effect
 * quality and the own images line with Manage images (FR-8.15, FR-8.75).
 */
export function BackgroundCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const effectHint = useId();
  const ownImages = useOwnImages();
  const [managing, setManaging] = useState(false);
  const builtinIds = (ownImages.library?.builtin ?? []).map((image) => image.id);
  const images = ownImages.library?.own ?? [];
  const ownIds = images.map((image) => image.id);
  const background = settings.cameraDefaultBackground;

  const options: { value: BackgroundChoice; label: string }[] = [
    { value: "none", label: t("camera.backgroundNone") },
    { value: "blur", label: t("camera.backgroundBlur") },
    ...(builtinIds.length > 0 || background.kind === "builtin"
      ? [{ value: "library" as const, label: t("camera.backgroundLibrary") }]
      : []),
    ...images.map((image) => ({
      value: `own:${image.id}` as const,
      label: t("camera.backgroundOwn", {
        name: t("camera.imageName", { number: ownImageNumber(images, image.id) }),
      }),
    })),
  ];

  const chooseBackground = (choice: BackgroundChoice) => {
    const next = backgroundOf(choice, background, builtinIds);
    if (next === null) return;
    update({ cameraDefaultBackground: next });
    applyToPreview(next, settings.cameraBlurStrength);
  };

  return (
    <Group title={t("camera.backgroundTitle")} icon="personBackground">
      <SelectRow
        label={t("camera.defaultBackground")}
        value={choiceOf(background, ownIds)}
        options={options}
        onChange={chooseBackground}
      />
      <BlurStrengthButtons
        value={settings.cameraBlurStrength}
        onChange={(cameraBlurStrength) => {
          update({ cameraBlurStrength });
          applyToPreview(background, cameraBlurStrength);
        }}
      />
      <SelectRow
        label={t("camera.effectQuality")}
        value={settings.cameraEffectQuality}
        describedBy={effectHint}
        options={EFFECT_QUALITIES.map((value) => ({ value, label: t(EFFECT_KEYS[value]) }))}
        onChange={(cameraEffectQuality) => {
          update({ cameraEffectQuality });
          getApi()
            .cameraSetEffectQuality(cameraEffectQuality)
            .catch((error: unknown) => {
              console.error("Changing the preview effect quality failed", error);
            });
        }}
      />
      <p id={effectHint} className="settings-hint">
        {t("camera.effectHint")}
      </p>
      <p className="settings-infoline">
        <InfoLineIcon name="lock" />
        <span>
          {t("camera.ownImages", { count: images.length })}{" "}
          <button
            type="button"
            className="settings-link"
            onClick={() => {
              setManaging(true);
            }}
          >
            {t("camera.manageImages")}
          </button>
        </span>
      </p>
      {managing && (
        <ManageImagesDialog
          ownImages={ownImages}
          onDeleted={(id) => {
            // Deleting the default background resets it to Blur (FR-8.15).
            if (background.kind === "own" && background.id === id) {
              update({ cameraDefaultBackground: { kind: "blur" } });
            }
          }}
          onClose={() => {
            setManaging(false);
          }}
        />
      )}
    </Group>
  );
}
