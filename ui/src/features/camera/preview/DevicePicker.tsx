/** The **Camera** picker of the preview (FR-8.11): switches the running preview and remembers it. */
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type CameraDevice } from "../../../api";
import { saveCameraSettings } from "./cameraSettings";
import { cameraFailureOf, failureText, initialDevice, type DesktopOs } from "./previewLogic";

export interface DevicePickerProps {
  devices: readonly CameraDevice[];
  /** The remembered camera; the system default when it is `null` or not connected. */
  initialId: string | null;
  os: DesktopOs | null;
  disabled: boolean;
  /** Why the chosen camera could not start; the old one keeps running (G-1). */
  onFailure: (text: string | null) => void;
}

export function DevicePicker({ devices, initialId, os, disabled, onFailure }: DevicePickerProps) {
  const { t } = useTranslation("camera");
  const id = useId();
  const [selected, setSelected] = useState(() => initialDevice(devices, initialId)?.id ?? "");

  const choose = (next: string) => {
    const before = selected;
    setSelected(next);
    onFailure(null);
    getApi()
      .cameraSetDevice(next)
      .then(
        () => saveCameraSettings({ cameraDeviceId: next }),
        (error: unknown) => {
          setSelected(before);
          onFailure(failureText(t, cameraFailureOf(error), os));
        },
      )
      .catch((error: unknown) => {
        console.error("Saving the camera choice failed", error);
      });
  };

  return (
    <div className="cam-field">
      <label className="cam-field__label" htmlFor={id}>
        {t("preview.camera")}
      </label>
      {devices.length === 0 ? (
        <p className="cam-field__empty">{t("noCameraFound")}</p>
      ) : (
        <select
          id={id}
          className="settings-input settings-input--compact"
          value={selected}
          disabled={disabled}
          onChange={(event) => {
            choose(event.target.value);
          }}
        >
          {devices.map((device) => (
            <option key={device.id} value={device.id}>
              {device.name}
            </option>
          ))}
        </select>
      )}
    </div>
  );
}
