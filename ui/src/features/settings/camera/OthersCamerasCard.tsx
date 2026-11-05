import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { CameraShow } from "../../../api";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import { SelectRow } from "../sections/ScreenSharingSection";
import type { SettingsSectionProps } from "../sections/types";
import { reduceMotionOn } from "./cameraSettings";

const SHOW: readonly CameraShow[] = ["always", "onOpen", "never"];
const SHOW_KEYS = {
  always: "camera.showAlways",
  onOpen: "camera.showOnOpen",
  never: "camera.showNever",
} as const;

function systemReducesMotion(): boolean {
  return typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** "Other people's cameras" card of Settings → Camera (mockup 11, FR-8.56, FR-8.75). */
export function OthersCamerasCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const showHint = useId();
  return (
    <Group title={t("camera.othersTitle")} icon="signal">
      <SelectRow
        label={t("camera.showCameras")}
        value={settings.cameraShow}
        describedBy={showHint}
        options={SHOW.map((value) => ({ value, label: t(SHOW_KEYS[value]) }))}
        onChange={(cameraShow) => {
          update({ cameraShow });
        }}
      />
      <p id={showHint} className="settings-hint">
        {t("camera.showHint")}
      </p>
      <Switch
        label={t("camera.dataSaver")}
        checked={settings.cameraDataSaver}
        onChange={(cameraDataSaver) => {
          update({ cameraDataSaver });
        }}
      />
      <Switch
        label={t("camera.reduceMotion")}
        checked={reduceMotionOn(settings.cameraReduceMotion, systemReducesMotion())}
        onChange={(on) => {
          update({ cameraReduceMotion: on ? "on" : "off" });
        }}
      />
      <p className="settings-hint">{t("camera.reduceMotionHint")}</p>
    </Group>
  );
}
