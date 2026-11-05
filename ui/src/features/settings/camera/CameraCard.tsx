import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type CameraDevice, type CameraSendQuality } from "../../../api";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import { SelectRow } from "../sections/ScreenSharingSection";
import type { SettingsSectionProps } from "../sections/types";
import { CameraPreviewTile } from "./CameraPreviewTile";
import { shownDeviceId } from "./cameraSettings";

const SEND_QUALITIES: readonly CameraSendQuality[] = ["auto", "720p15", "360p30"];
const SEND_QUALITY_KEYS = {
  auto: "camera.sendQualityAuto",
  "720p15": "camera.sendQuality720",
  "360p30": "camera.sendQuality360",
} as const;

/** The cameras of this PC; empty while loading or without any. */
function useCameraDevices(): readonly CameraDevice[] {
  const [devices, setDevices] = useState<readonly CameraDevice[]>([]);
  useEffect(() => {
    let active = true;
    getApi()
      .cameraDevices()
      .then(
        (list) => {
          if (active) setDevices(list);
        },
        (error: unknown) => {
          console.error("Listing the cameras failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);
  return devices;
}

/** Camera card of Settings → Camera (mockup 11): live preview, device, send quality, mirror. */
export function CameraCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const qualityHint = useId();
  const devices = useCameraDevices();

  const chooseDevice = (cameraDeviceId: string) => {
    update({ cameraDeviceId });
    getApi()
      .cameraSetDevice(cameraDeviceId)
      .catch((error: unknown) => {
        console.error("Switching the preview camera failed", error);
      });
  };

  return (
    <Group title={t("camera.cameraTitle")} icon="camera">
      <CameraPreviewTile mirror={settings.cameraMirrorSelf} />
      <SelectRow
        label={t("camera.device")}
        value={shownDeviceId(devices, settings.cameraDeviceId)}
        options={
          devices.length === 0
            ? [{ value: "", label: t("camera.noDevice") }]
            : devices.map((device) => ({ value: device.id, label: device.name }))
        }
        onChange={(id) => {
          if (id !== "") chooseDevice(id);
        }}
      />
      <SelectRow
        label={t("camera.sendQuality")}
        value={settings.cameraSendQuality}
        describedBy={qualityHint}
        options={SEND_QUALITIES.map((value) => ({ value, label: t(SEND_QUALITY_KEYS[value]) }))}
        onChange={(cameraSendQuality) => {
          update({ cameraSendQuality });
        }}
      />
      <p id={qualityHint} className="settings-hint">
        {t("camera.sendQualityHint")}
      </p>
      <Switch
        label={t("camera.mirror")}
        checked={settings.cameraMirrorSelf}
        onChange={(cameraMirrorSelf) => {
          update({ cameraMirrorSelf });
        }}
      />
      <p className="settings-hint">{t("camera.mirrorHint")}</p>
    </Group>
  );
}
