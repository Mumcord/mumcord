import { useCallback, useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type AudioDevice, type AudioDevices } from "../../../api";
import { LevelMeter } from "../components/LevelMeter";
import { Group, Section } from "../components/Section";
import { Switch } from "../components/Switch";
import { readableDeviceName } from "../deviceNames";
import { errorText } from "../errors";
import type { SettingsSectionProps } from "./types";

type MicTestPhaseName = "recording" | "playing" | "done" | "failed";

/** Audio: input/output device, level meter and record-then-play-back test. */
export function AudioSection({ settings, update }: SettingsSectionProps) {
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
    <Section title={t("sections.audio")}>
      <Group title={t("audio.devices")}>
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
      <Group title={t("audio.quality")}>
        <Switch
          label={t("audio.musicQuality")}
          description={t("audio.musicQualityHint")}
          checked={settings.musicQuality}
          onChange={(musicQuality) => {
            update({ musicQuality });
          }}
        />
      </Group>
      <MicTest />
    </Section>
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
    <div className="settings-field-row">
      <label className="settings-field" htmlFor={id}>
        {label}
      </label>
      <select
        id={id}
        className="settings-input"
        value={value ?? DEFAULT_VALUE}
        onChange={(event) => {
          onChange(event.target.value === DEFAULT_VALUE ? null : event.target.value);
        }}
      >
        <option value={DEFAULT_VALUE}>{t("audio.systemDefault")}</option>
        {devices.map((device) => (
          <option key={device.id} value={device.id}>
            {readableDeviceName(device.name)}
          </option>
        ))}
        {missing && <option value={value}>{t("audio.unavailableDevice")}</option>}
      </select>
    </div>
  );
}

function MicTest() {
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

  // Leaving the section ends a running test.
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
      await getApi().micTestStart();
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
    <Group title={t("audio.test")}>
      <p className="settings-hint">{t("audio.testHint")}</p>
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
