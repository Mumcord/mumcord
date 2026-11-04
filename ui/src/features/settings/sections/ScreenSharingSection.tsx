import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  getApi,
  type EncoderStatus,
  type GpuVendor,
  type ScreenCodec,
  type ScreenFps,
  type ScreenOptimize,
  type ScreenResolution,
  type ScreenUploadLimit,
} from "../../../api";
import type { VideoCodec } from "../../../bindings/commands";
import { webCodecsCodecs } from "../../../api/screenFrames";
import { Group } from "../components/Section";
import { SettingsIcon } from "../components/SettingsIcon";
import { Switch } from "../components/Switch";
import { errorText } from "../errors";
import type { SettingsSectionProps } from "./types";

// The 4K option stays hidden (mockup 05), so the engine's list is not offered as a whole.
const RESOLUTIONS: readonly ScreenResolution[] = ["auto", "720", "1080", "1440"];
const FPS: readonly ScreenFps[] = ["15", "30", "60"];
const OPTIMIZE: readonly ScreenOptimize[] = ["balanced", "text", "motion"];
const UPLOAD_LIMITS: readonly ScreenUploadLimit[] = ["auto", "2", "5", "10", "20"];
const CODECS: readonly ScreenCodec[] = ["auto", "av1", "vp9"];

const OPTIMIZE_KEYS = {
  balanced: "screenShare.optimizeBalanced",
  text: "screenShare.optimizeText",
  motion: "screenShare.optimizeMotion",
} as const;

const CODEC_KEYS = {
  auto: ["screenShare.codecAuto", "screenShare.codecAutoHint"],
  av1: ["screenShare.codecAv1", "screenShare.codecAv1Hint"],
  vp9: ["screenShare.codecVp9", "screenShare.codecVp9Hint"],
} as const;

const GPU_KEYS = {
  nvidia: "screenShare.gpuNvidia",
  amd: "screenShare.gpuAmd",
  intel: "screenShare.gpuIntel",
  unknown: "screenShare.gpuUnknown",
} as const;

/** Select with a visible label, as the other settings rows. */
function SelectRow<T extends string>({
  label,
  value,
  options,
  describedBy,
  onChange,
}: {
  label: string;
  value: T;
  options: readonly { value: T; label: string }[];
  describedBy?: string;
  onChange: (value: T) => void;
}) {
  const id = useId();
  return (
    <div className="settings-control-row">
      <label htmlFor={id}>{label}</label>
      <select
        id={id}
        className="settings-input settings-input--compact"
        value={value}
        aria-describedby={describedBy}
        onChange={(event) => {
          const next = options.find((option) => option.value === event.target.value);
          if (next !== undefined) onChange(next.value);
        }}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </div>
  );
}

/** Default Resolution, Frame rate, Optimize for and Upload limit of a new share (FR-7.60). */
export function QualityDefaultsCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const resolutionHint = useId();
  const uploadHint = useId();
  return (
    <Group title={t("screenShare.qualityTitle")} icon="gauge">
      <SelectRow
        label={t("screenShare.resolution")}
        value={settings.screenResolution}
        describedBy={resolutionHint}
        options={RESOLUTIONS.map((value) => ({
          value,
          label: value === "auto" ? t("screenShare.resolutionAuto") : `${value}p`,
        }))}
        onChange={(screenResolution) => {
          update({ screenResolution });
        }}
      />
      <p id={resolutionHint} className="settings-hint">
        {t("screenShare.resolutionHint")}
      </p>
      <SelectRow
        label={t("screenShare.fps")}
        value={settings.screenFps}
        options={FPS.map((value) => ({ value, label: t("screenShare.fpsOption", { fps: value }) }))}
        onChange={(screenFps) => {
          update({ screenFps });
        }}
      />
      <SelectRow
        label={t("screenShare.optimize")}
        value={settings.screenOptimize}
        options={OPTIMIZE.map((value) => ({ value, label: t(OPTIMIZE_KEYS[value]) }))}
        onChange={(screenOptimize) => {
          update({ screenOptimize });
        }}
      />
      <SelectRow
        label={t("screenShare.uploadLimit")}
        value={settings.screenUploadLimit}
        describedBy={uploadHint}
        options={UPLOAD_LIMITS.map((value) => ({
          value,
          label:
            value === "auto"
              ? t("screenShare.uploadAuto")
              : t("screenShare.uploadOption", { mbit: value }),
        }))}
        onChange={(screenUploadLimit) => {
          update({ screenUploadLimit });
        }}
      />
      <p id={uploadHint} className="settings-hint">
        {t("screenShare.uploadHint")}
      </p>
      <Switch
        label={t("screenShare.askEachTime")}
        checked={settings.screenAskEachTime}
        onChange={(screenAskEachTime) => {
          update({ screenAskEachTime });
        }}
      />
    </Group>
  );
}

/** Codec choice: Automatic, AV1 or VP9, each with its hint (FR-7.60). */
export function CodecCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const name = useId();
  return (
    <Group title={t("screenShare.codecTitle")} icon="monitor">
      <fieldset className="settings-radios settings-radios--hinted">
        <legend className="settings-radios__legend">
          <span>{t("screenShare.codec")}</span>
        </legend>
        {CODECS.map((value) => {
          const [labelKey, hintKey] = CODEC_KEYS[value];
          const hintId = `${name}-${value}`;
          return (
            <div key={value} className="settings-radios__row">
              <label className="settings-radios__option">
                <input
                  type="radio"
                  name={name}
                  checked={settings.screenCodec === value}
                  aria-describedby={hintId}
                  onChange={() => {
                    update({ screenCodec: value });
                  }}
                />
                {t(labelKey)}
              </label>
              <span id={hintId} className="settings-hint">
                {t(hintKey)}
              </span>
            </div>
          );
        })}
      </fieldset>
      <p className="settings-infoline">
        <SettingsIcon name="info" />
        {t("screenShare.codecInfo")}
      </p>
      <p className="settings-hint">{t("screenShare.codecOlder")}</p>
    </Group>
  );
}

/** Live encoder status: loaded once, then kept current by the engine's event. */
function useEncoderStatus(): EncoderStatus | null {
  const [status, setStatus] = useState<EncoderStatus | null>(null);
  useEffect(() => {
    let active = true;
    const api = getApi();
    api.screenEncoderStatus().then(
      (loaded) => {
        if (active) setStatus((current) => current ?? loaded);
      },
      (error: unknown) => {
        console.error("Reading the encoder status failed", error);
      },
    );
    const unlisten = api.on("screenEncoderStatusChanged", (event) => {
      if (active) setStatus(event.status);
    });
    return () => {
      active = false;
      unlisten();
    };
  }, []);
  return status;
}

/** Codecs this webview plays itself; the engine's software decoder covers the rest. */
function useWebCodecs(): readonly VideoCodec[] {
  const [codecs, setCodecs] = useState<readonly VideoCodec[]>([]);
  useEffect(() => {
    let active = true;
    webCodecsCodecs().then(
      (found) => {
        if (active) setCodecs(found);
      },
      (error: unknown) => {
        console.error("Checking the playback support failed", error);
      },
    );
    return () => {
      active = false;
    };
  }, []);
  return codecs;
}

function EncoderRow({ label, value, ok }: { label: string; value: string; ok: boolean }) {
  return (
    <li className="settings-hwlist__row">
      <span>{label}</span>
      <span className="settings-hwlist__value" data-ok={ok ? "" : undefined}>
        <SettingsIcon name={ok ? "tick" : "info"} />
        {value}
      </span>
    </li>
  );
}

/** "Use graphics card" switch, what this PC can encode and the "Check again" action. */
export function HardwareCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const status = useEncoderStatus();
  const playable = useWebCodecs();
  const playback = (codec: VideoCodec) =>
    t(playable.includes(codec) ? "screenShare.playbackHardware" : "screenShare.playbackSoftware");
  const [error, setError] = useState<string | null>(null);

  const gpu = (vendor: GpuVendor) => t(GPU_KEYS[vendor]);
  const activeLine = (current: EncoderStatus): string => {
    const { codec, gpu: vendor } = current.active;
    if (codec === "av1") {
      return vendor === null
        ? t("screenShare.activeAv1Cpu")
        : t("screenShare.activeAv1Gpu", { gpu: gpu(vendor) });
    }
    if (current.av1Skipped === "tooHeavy") return t("screenShare.skippedTooHeavy");
    if (current.av1Skipped === "onBattery") return t("screenShare.skippedOnBattery");
    if (current.av1Skipped === "aboveSoftwareLimit") return t("screenShare.skippedAboveLimit");
    return vendor === null
      ? t("screenShare.activeVp9Cpu")
      : t("screenShare.activeVp9Gpu", { gpu: gpu(vendor) });
  };

  const checkAgain = () => {
    setError(null);
    getApi()
      .screenEncoderCheckAgain()
      .catch((cause: unknown) => {
        console.error("Checking the encoders again failed", cause);
        setError(errorText(t, cause));
      });
  };

  return (
    <Group title={t("screenShare.hardwareTitle")} icon="cpu">
      <Switch
        label={t("screenShare.useGpu")}
        status={t("screenShare.useGpuHint")}
        checked={settings.screenUseGpu}
        onChange={(screenUseGpu) => {
          update({ screenUseGpu });
        }}
      />
      {status !== null && (
        <>
          <p className="settings-hint" role="status">
            {status.checking
              ? t("screenShare.checking")
              : t("screenShare.active", { encoder: activeLine(status) })}
          </p>
          <ul className="settings-hwlist" aria-label={t("screenShare.statusList")}>
            <EncoderRow
              label={t("screenShare.av1Encode")}
              ok={status.av1Hardware !== null}
              value={
                status.av1Hardware === null
                  ? t("screenShare.notAvailable")
                  : gpu(status.av1Hardware)
              }
            />
            <EncoderRow
              label={t("screenShare.av1EncodeCpu")}
              ok={status.av1SoftwareLimit !== null}
              value={
                status.av1SoftwareLimit === null
                  ? t("screenShare.notAvailable")
                  : t("screenShare.softwareLimit", {
                      height: status.av1SoftwareLimit.height,
                      fps: status.av1SoftwareLimit.fps,
                    })
              }
            />
            <EncoderRow
              label={t("screenShare.av1Playback")}
              ok={playable.includes("av1")}
              value={playback("av1")}
            />
            <EncoderRow
              label={t("screenShare.vp9Encode")}
              ok={status.vp9Hardware !== null}
              value={
                status.vp9Hardware === null ? t("screenShare.software") : gpu(status.vp9Hardware)
              }
            />
            <EncoderRow
              label={t("screenShare.vp9Playback")}
              ok={playable.includes("vp9")}
              value={playback("vp9")}
            />
          </ul>
          {status.softwareWarning && (
            <p className="settings-hint">{t("screenShare.softwareWarning")}</p>
          )}
        </>
      )}
      {error !== null && (
        <p className="settings-error" role="alert">
          {error}
        </p>
      )}
      <div className="settings-group__actions">
        <button
          type="button"
          className="settings-button"
          disabled={status?.checking === true}
          onClick={checkAgain}
        >
          {t("screenShare.checkAgain")}
        </button>
      </div>
    </Group>
  );
}
