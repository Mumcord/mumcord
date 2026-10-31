import { useCallback, useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type AudioDevice, type AudioDevices } from "../../../api";
import { Select } from "../../../components/Select";
import { LevelMeter } from "../components/LevelMeter";
import { Group } from "../components/Section";
import { readableDeviceName } from "../deviceNames";
import { errorText } from "../errors";
import type { SettingsSectionProps } from "./types";

type MicTestPhaseName = "recording" | "playing" | "done" | "failed";

/** Microphone and speakers; follows devices that are plugged in or out. */
export function DevicesCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const [devices, setDevices] = useState<AudioDevices | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    const api = getApi();
    api.audioDevices().then(
      (loaded) => {
        if (active) setDevices(loaded);
      },
      (error: unknown) => {
        console.error("Loading audio devices failed", error);
        if (active) setProblem(errorText(t, error));
      },
    );
    const unsubscribe = api.on("audioDevicesChanged", ({ devices: changed }) => {
      setDevices(changed);
    });
    return () => {
      active = false;
      unsubscribe();
    };
  }, [t]);

  return (
    <Group title={t("audio.devices")} icon="headphones" wide>
      <DeviceSelect
        label={t("audio.input")}
        devices={devices?.inputs ?? []}
        value={settings.inputDevice}
        onChange={(inputDevice) => {
          update({ inputDevice });
        }}
      />
      <DeviceSelect
        label={t("audio.output")}
        devices={devices?.outputs ?? []}
        value={settings.outputDevice}
        onChange={(outputDevice) => {
          update({ outputDevice });
        }}
      />
      {problem !== null && (
        <p role="alert" className="settings-error">
          {problem}
        </p>
      )}
    </Group>
  );
}

interface DeviceSelectProps {
  label: string;
  devices: AudioDevice[];
  value: string | null;
  onChange: (id: string | null) => void;
}

const DEFAULT_VALUE = "";

function DeviceSelect({ label, devices, value, onChange }: DeviceSelectProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  const missing = value !== null && !devices.some((device) => device.id === value);
  return (
    <div className="settings-control-row">
      <label htmlFor={id}>{label}</label>
      <Select
        id={id}
        value={value ?? DEFAULT_VALUE}
        options={[
          { value: DEFAULT_VALUE, label: t("audio.systemDefault") },
          ...devices.map((device) => ({
            value: device.id,
            label: readableDeviceName(device.name),
          })),
          ...(missing ? [{ value, label: t("audio.unavailableDevice") }] : []),
        ]}
        onChange={(next) => {
          onChange(next === DEFAULT_VALUE ? null : next);
        }}
      />
    </div>
  );
}

/** Level meter and the record-then-play-back test. */
export function MicTestCard() {
  const { t } = useTranslation("settings");
  const [running, setRunning] = useState(false);
  const [level, setLevel] = useState(0);
  const [phase, setPhase] = useState<MicTestPhaseName>("recording");
  const [problem, setProblem] = useState<string | null>(null);
  const runningRef = useRef(false);
  const mounted = useRef(true);

  useEffect(
    () =>
      getApi().on("micLevel", (payload) => {
        setLevel(payload.level ?? 0);
      }),
    [],
  );

  const stop = useCallback(() => {
    if (!runningRef.current) return;
    runningRef.current = false;
    setRunning(false);
    getApi()
      .micTestStop()
      .catch((error: unknown) => {
        console.error("Stopping the mic test failed", error);
      });
  }, []);

  // The engine ends the test on its own: recording, then playback, then `done` (or `failed`).
  useEffect(
    () =>
      getApi().on("micTestPhaseChanged", (payload) => {
        setPhase(payload.phase);
        if (payload.phase === "done" || payload.phase === "failed") {
          runningRef.current = false;
          setRunning(false);
          setLevel(0);
        }
      }),
    [],
  );

  // Leaving the page ends a running test.
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      stop();
    };
  }, [stop]);

  const start = async () => {
    setProblem(null);
    setPhase("recording");
    try {
      await getApi().micTestStart(null);
      runningRef.current = true;
      if (!mounted.current) {
        stop();
        return;
      }
      setRunning(true);
    } catch (error: unknown) {
      console.error("Starting the mic test failed", error);
      setProblem(errorText(t, error));
    }
  };

  return (
    <Group title={t("audio.test")} icon="waveform" info={t("audio.testHint")}>
      <LevelMeter label={t("audio.level")} level={level} />
      <div className="settings-actions">
        {running ? (
          <button type="button" className="settings-button" onClick={stop}>
            {t("audio.testStop")}
          </button>
        ) : (
          <button type="button" className="settings-button" onClick={() => void start()}>
            {t("audio.testStart")}
          </button>
        )}
      </div>
      <p role="status" className="settings-hint">
        {running ? t(phase === "playing" ? "audio.testPlaying" : "audio.testRecording") : ""}
      </p>
      {problem !== null && (
        <p role="alert" className="settings-error">
          {problem}
        </p>
      )}
    </Group>
  );
}
