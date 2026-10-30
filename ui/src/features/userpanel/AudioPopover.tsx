/**
 * Quick audio popover on the mute and deafen buttons (#153): the device list, a gain or volume
 * slider and a few related controls. Every change goes through the same settings as the Voice
 * settings (applied and saved at once), so there is no second source of truth.
 */
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { getApi, type AudioDevice, type Settings } from "../../api";
import type { MenuPosition } from "../../components/ContextMenu";
import { useThrottledSend } from "../../components/useThrottledSend";
import { readableDeviceName } from "../settings/deviceNames";
import { useSettings } from "../settings/useSettings";
import { SYSTEM_DEFAULT, selectedDeviceId } from "./selectedDevice";
import { useAudioDevices } from "./useAudioDevices";
import { useLevelBar } from "./useLevelBar";
import "./audiopopover.css";

/** Gap kept between the popover and the window edge. */
const EDGE = 8;
/** Slider changes are saved at most this often while dragging. */
export const SLIDER_THROTTLE_MS = 100;
const INPUT_GAIN_MAX = 200;
const OUTPUT_VOLUME_MAX = 100;

export type AudioKind = "input" | "output";

export interface AudioAnchor extends MenuPosition {
  /** Top edge of the control, so the popover can open upwards when there is no room below. */
  top: number;
}

interface AudioPopoverProps {
  kind: AudioKind;
  anchor: AudioAnchor;
  onClose: () => void;
  onVoiceSettings: () => void;
}

export function AudioPopover({ kind, anchor, onClose, onVoiceSettings }: AudioPopoverProps) {
  const { t } = useTranslation("userpanel");
  const ref = useRef<HTMLDivElement>(null);
  const { settings, update } = useSettings();
  const devices = useAudioDevices();
  const input = kind === "input";
  const list = (input ? devices?.inputs : devices?.outputs) ?? [];
  const chosen = settings ? (input ? settings.inputDevice : settings.outputDevice) : null;
  const selected = selectedDeviceId(list, chosen);
  const title = input ? t("audio.inputDevices") : t("audio.outputDevices");

  const close = useCallback(
    (returnFocus: boolean) => {
      onClose();
      if (returnFocus && anchor.opener?.isConnected) anchor.opener.focus();
    },
    [onClose, anchor],
  );

  useEffect(() => {
    const dismiss = () => {
      onClose();
    };
    const onPointerDown = (event: PointerEvent) => {
      if (!(event.target instanceof Node && ref.current?.contains(event.target))) dismiss();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("blur", dismiss);
    window.addEventListener("resize", dismiss);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("blur", dismiss);
      window.removeEventListener("resize", dismiss);
    };
  }, [onClose]);

  const ready = settings !== null && devices !== null;
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const left = Math.max(EDGE, Math.min(anchor.x, window.innerWidth - element.offsetWidth - EDGE));
    // The voice bar sits at the bottom of the window, so open upwards when it does not fit below.
    const fitsBelow = anchor.y + element.offsetHeight + EDGE <= window.innerHeight;
    const wanted = fitsBelow ? anchor.y : anchor.top - element.offsetHeight;
    const top = Math.max(EDGE, Math.min(wanted, window.innerHeight - element.offsetHeight - EDGE));
    element.style.left = `${String(left)}px`;
    element.style.top = `${String(top)}px`;
  }, [anchor, ready, list.length]);

  // Focus the checked device (else the first control) once the lists are there.
  useEffect(() => {
    if (!ready) return;
    const root = ref.current;
    root?.querySelector<HTMLElement>('[role="option"][aria-selected="true"]')?.focus();
    if (!root?.contains(document.activeElement))
      root?.querySelector<HTMLElement>("button, input")?.focus();
  }, [ready]);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close(true);
    }
  }

  // Leaving the popover with Tab closes it, like the other menus; focus stays where it moved.
  function onBlur(event: React.FocusEvent<HTMLDivElement>) {
    const next = event.relatedTarget;
    if (next instanceof Node && !ref.current?.contains(next)) onClose();
  }

  const choose = (id: string) => {
    // "System default" is stored as "follow the system default" (null).
    const value = id === SYSTEM_DEFAULT ? null : id;
    update(input ? { inputDevice: value } : { outputDevice: value });
  };

  return createPortal(
    // The dialog only listens for Escape and focus leaving; both reach it from the controls inside.
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions
    <div
      ref={ref}
      className="audio-popover"
      role="dialog"
      aria-label={title}
      onKeyDown={onKeyDown}
      onBlur={onBlur}
    >
      <p className="audio-popover__title">{title}</p>
      <DeviceList
        label={title}
        devices={list}
        selected={selected}
        loaded={devices !== null}
        onChoose={choose}
      />
      {settings ? (
        input ? (
          <InputControls settings={settings} update={update} />
        ) : (
          <OutputControls settings={settings} update={update} />
        )
      ) : null}
      <button
        type="button"
        className="audio-popover__link"
        onClick={() => {
          onClose();
          onVoiceSettings();
        }}
      >
        {t("audio.voiceSettings")}
        <span aria-hidden="true">→</span>
      </button>
    </div>,
    document.body,
  );
}

interface DeviceListProps {
  label: string;
  devices: readonly AudioDevice[];
  selected: string;
  loaded: boolean;
  onChoose: (id: string) => void;
}

const readableName = (device: AudioDevice | undefined) =>
  device ? readableDeviceName(device.name) : null;

function DeviceList({ label, devices, selected, loaded, onChoose }: DeviceListProps) {
  const { t } = useTranslation("userpanel");
  const ref = useRef<HTMLDivElement>(null);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const items = [...(ref.current?.querySelectorAll<HTMLElement>('[role="option"]') ?? [])];
    const index = items.findIndex((item) => item === document.activeElement);
    const focusAt = (next: number) => {
      items[(next + items.length) % items.length]?.focus();
    };
    switch (event.key) {
      case "ArrowDown":
        focusAt(index + 1);
        break;
      case "ArrowUp":
        focusAt(index - 1);
        break;
      case "Home":
        focusAt(0);
        break;
      case "End":
        focusAt(items.length - 1);
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  // The system default row comes last and names the device it currently stands for.
  const rows = [
    ...devices.map((device) => ({
      id: device.id,
      name: readableDeviceName(device.name),
      hint: null as string | null,
    })),
    {
      id: SYSTEM_DEFAULT,
      name: t("audio.systemDefault"),
      hint: readableName(devices.find((device) => device.isDefault)),
    },
  ];
  if (loaded && devices.length === 0) {
    return <p className="audio-popover__empty">{t("audio.noDevices")}</p>;
  }
  return (
    <div
      ref={ref}
      className="audio-popover__devices"
      role="listbox"
      tabIndex={-1}
      aria-label={label}
      onKeyDown={onKeyDown}
    >
      {rows.map((row) => {
        const checked = row.id === selected;
        return (
          <div
            key={row.id}
            role="option"
            aria-selected={checked}
            tabIndex={checked ? 0 : -1}
            className="audio-popover__device"
            onClick={() => {
              onChoose(row.id);
            }}
            onKeyDown={(event) => {
              if (event.key !== "Enter" && event.key !== " ") return;
              event.preventDefault();
              onChoose(row.id);
            }}
          >
            <span className="audio-popover__check" aria-hidden="true">
              {checked ? "✓" : ""}
            </span>
            <span className="audio-popover__name">{row.name}</span>
            {row.hint !== null ? <span className="audio-popover__badge">{row.hint}</span> : null}
          </div>
        );
      })}
    </div>
  );
}

interface ControlsProps {
  settings: Settings;
  update: (patch: Partial<Settings>) => void;
}

function InputControls({ settings, update }: ControlsProps) {
  const { t } = useTranslation("userpanel");
  const switchId = useId();
  const levelRef = useLevelBar();
  return (
    <>
      <PercentSlider
        label={t("audio.volume")}
        value={settings.inputGainPercent}
        max={INPUT_GAIN_MAX}
        onChange={(inputGainPercent) => {
          update({ inputGainPercent });
        }}
      />
      {/* Decoration only: the level is not announced to screen readers. */}
      <div className="audio-popover__level" aria-hidden="true">
        <span>{t("audio.level")}</span>
        <div className="audio-popover__meter">
          <div ref={levelRef} className="audio-popover__meter-fill" />
        </div>
      </div>
      <div className="audio-popover__switch">
        <label htmlFor={switchId}>{t("audio.noiseSuppression")}</label>
        <input
          id={switchId}
          type="checkbox"
          role="switch"
          checked={settings.noiseSuppression}
          onChange={(event) => {
            update({ noiseSuppression: event.target.checked });
          }}
        />
      </div>
    </>
  );
}

function OutputControls({ settings, update }: ControlsProps) {
  const { t } = useTranslation("userpanel");
  return (
    <>
      <PercentSlider
        label={t("audio.volume")}
        value={settings.outputVolumePercent}
        max={OUTPUT_VOLUME_MAX}
        onChange={(outputVolumePercent) => {
          update({ outputVolumePercent });
        }}
      />
      <button
        type="button"
        className="audio-popover__action"
        onClick={() => {
          getApi()
            .audioTestSound()
            .catch((error: unknown) => {
              console.error("Playing the test sound failed", error);
            });
        }}
      >
        {t("audio.testSound")}
        <span aria-hidden="true">▶</span>
      </button>
    </>
  );
}

interface PercentSliderProps {
  label: string;
  value: number;
  max: number;
  onChange: (percent: number) => void;
}

/** A percent slider that follows the pointer at once and saves throttled while dragging. */
function PercentSlider({ label, value, max, onChange }: PercentSliderProps) {
  const id = useId();
  const { schedule } = useThrottledSend(SLIDER_THROTTLE_MS);
  const [dragging, setDragging] = useState<number | null>(null);
  const shown = Math.min(dragging ?? value, max);
  return (
    <div className="audio-popover__slider">
      <label htmlFor={id}>{label}</label>
      <input
        id={id}
        type="range"
        min={0}
        max={max}
        step={5}
        value={shown}
        aria-valuetext={`${String(shown)}%`}
        onChange={(event) => {
          const next = Number(event.target.value);
          setDragging(next);
          schedule(() => {
            onChange(next);
          });
        }}
        onPointerUp={() => {
          setDragging(null);
        }}
        onBlur={() => {
          setDragging(null);
        }}
        onKeyUp={() => {
          setDragging(null);
        }}
      />
      <span aria-hidden="true">{`${String(shown)}%`}</span>
    </div>
  );
}
