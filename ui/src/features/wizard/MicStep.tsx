import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type AudioDevice, type AudioDevices, type Settings } from "../../api";
import { Select } from "../../components/Select";
import { shortDeviceName } from "./deviceName";
import { LevelMeter } from "./LevelMeter";
import { StepFrame } from "./StepFrame";

type TestPhase = "idle" | "recording" | "playing";

interface MicStepProps {
  /** 1-based step number and step count of this wizard pass (#149 rerun: 1 of 4). */
  step: number;
  total: number;
  settings: Settings;
  focusOnMount: boolean;
  /**
   * Reports the changed devices. First start saves them at once so the test uses them; the
   * rerun keeps them as a draft until Finish (#149), and the test then uses the draft devices.
   */
  onDevicesChange: (devices: Pick<Settings, "inputDevice" | "outputDevice">) => Promise<void>;
  /** The shown devices are not saved yet: the test uses them without writing settings. */
  stagedDevices?: boolean;
  /** Rerun step 1 is the first step: no Back there. */
  onBack?: (() => void) | undefined;
  onNext: () => void;
  /** Rerun only: closes the wizard (#149). */
  onCancel?: (() => void) | undefined;
}

/** Step 2 on first start, step 1 in the rerun: devices, level meter and record-then-play test (FR-1.4). */
export function MicStep({
  step,
  total,
  settings,
  focusOnMount,
  onDevicesChange,
  stagedDevices = false,
  onBack,
  onNext,
  onCancel,
}: MicStepProps) {
  const { t } = useTranslation("wizard");
  const [devices, setDevices] = useState<AudioDevices | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [phase, setPhase] = useState<TestPhase>("idle");
  const [level, setLevel] = useState(0);
  const running = useRef(false);
  const inputId = useId();
  const outputId = useId();

  useEffect(() => {
    let active = true;
    getApi()
      .audioDevices()
      .then((list) => {
        if (active) setDevices(list);
      })
      .catch((cause: unknown) => {
        console.error("listing audio devices failed", cause);
        if (active) setError(t("mic.devicesFailed"));
      });
    const offDevices = getApi().on("audioDevicesChanged", (event) => {
      setDevices(event.devices);
    });
    const offLevel = getApi().on("micLevel", (event) => {
      if (running.current) setLevel(event.level ?? 0);
    });
    // The engine reports the test's progress: recording, then playback, then done.
    const offPhase = getApi().on("micTestPhaseChanged", (event) => {
      if (event.phase === "recording" || event.phase === "playing") {
        running.current = true;
        setPhase(event.phase);
        return;
      }
      running.current = false;
      setPhase("idle");
      setLevel(0);
      if (event.phase === "failed") setError(t("mic.testFailed"));
    });
    return () => {
      active = false;
      offDevices();
      offLevel();
      offPhase();
    };
  }, [t]);

  // Leaving the step (Back/Next or unmount) always ends a running test.
  useEffect(
    () => () => {
      if (running.current) {
        running.current = false;
        getApi()
          .micTestStop()
          .catch((cause: unknown) => {
            console.error("stopping the mic test failed", cause);
          });
      }
    },
    [],
  );

  async function stopTest() {
    running.current = false;
    setPhase("idle");
    setLevel(0);
    try {
      await getApi().micTestStop();
    } catch (cause: unknown) {
      console.error("stopping the mic test failed", cause);
    }
  }

  async function startTest() {
    setError(null);
    try {
      // Unsaved devices are tested on their own; the engine switches back afterwards (#149).
      await getApi().micTestStart(
        stagedDevices
          ? { inputDevice: settings.inputDevice, outputDevice: settings.outputDevice }
          : null,
      );
    } catch (cause: unknown) {
      console.error("starting the mic test failed", cause);
      setError(t("mic.testFailed"));
      return;
    }
    // Normally the engine's `recording` event already arrived; never move a phase backwards.
    running.current = true;
    setPhase((current) => (current === "idle" ? "recording" : current));
  }

  async function changeDevice(key: "inputDevice" | "outputDevice", value: string) {
    if (running.current) await stopTest();
    setError(null);
    try {
      await onDevicesChange({
        inputDevice: settings.inputDevice,
        outputDevice: settings.outputDevice,
        [key]: value === "" ? null : value,
      });
    } catch {
      setError(t("errors.saveFailed"));
    }
  }

  const label = (device: AudioDevice) =>
    shortDeviceName(device.name, (backend) => t("mic.defaultVia", { backend }));

  function deviceSelect(
    id: string,
    key: "inputDevice" | "outputDevice",
    list: AudioDevice[],
    caption: string,
  ) {
    const selected = settings[key];
    const value = selected !== null && list.some((d) => d.id === selected) ? selected : "";
    return (
      <div className="wizard-field">
        <label htmlFor={id}>{caption}</label>
        <Select
          id={id}
          value={value}
          options={[
            { value: "", label: t("mic.systemDefault") },
            ...list.map((device) => ({ value: device.id, label: label(device) })),
          ]}
          onChange={(next) => void changeDevice(key, next)}
        />
      </div>
    );
  }

  return (
    <StepFrame
      step={step}
      total={total}
      title={t("mic.title")}
      focusOnMount={focusOnMount}
      busy={false}
      submitLabel={t("common:next")}
      onSubmit={onNext}
      onBack={onBack}
      onCancel={onCancel}
    >
      <p className="wizard-muted">{t("mic.intro")}</p>
      {deviceSelect(inputId, "inputDevice", devices?.inputs ?? [], t("mic.input"))}
      {deviceSelect(outputId, "outputDevice", devices?.outputs ?? [], t("mic.output"))}
      <LevelMeter level={level} label={t("mic.level")} />
      <div className="wizard-row">
        <button
          type="button"
          className="button"
          onClick={() => void (phase === "idle" ? startTest() : stopTest())}
        >
          {phase === "idle" ? t("mic.test") : t("mic.stop")}
        </button>
        <p role="status" className="wizard-muted">
          {t(`mic.${phase}`)}
        </p>
      </div>
      {error && (
        <p role="alert" className="wizard-error">
          {error}
        </p>
      )}
    </StepFrame>
  );
}
