import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type UpdateStatus } from "../../../api";
import { Select } from "../../../components/Select";
import { openUpdateDialog, useUpdateStatus } from "../../updates";
import {
  effectiveUpdateMode,
  UPDATE_INTERVALS,
  UPDATE_MODES,
  updateModePatch,
  type UpdateMode,
} from "../../updates/updateMode";
import { Group, Section } from "../components/Section";
import type { SettingsSectionProps } from "./types";
import { lastChecked } from "./lastChecked";
import "./updates.css";

const MODE_LABELS = { auto: "modeAuto", notify: "modeNotify", off: "modeOff" } as const;

/** Updates (FR-3.144, FR-3.146): when Mumcord looks for new versions and what it does then. */
export function UpdatesSection({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const { status } = useUpdateStatus();
  const canInstall = status?.canInstall ?? true;
  const mode = effectiveUpdateMode(settings, canInstall);
  return (
    <Section title={t("sections.updates")}>
      {status !== null && (
        <p className="upd-settings-origin">
          {t("updates.installedFrom", {
            version: status.currentVersion,
            source: t(`updates.install.${status.install}`),
          })}
        </p>
      )}
      <Group title={t("updates.checking")}>
        <ModeRow
          mode={mode}
          canInstall={canInstall}
          onChange={(next) => {
            update(updateModePatch(next));
          }}
        />
        <IntervalRow
          hours={settings.updateIntervalHours}
          disabled={mode === "off"}
          onChange={(hours) => {
            update({ updateIntervalHours: hours });
          }}
        />
      </Group>
      <Group title={t("updates.status")}>
        <StatusRow status={status} off={mode === "off"} />
        {settings.updateSkipped !== null && (
          <div className="upd-settings-row">
            <div className="upd-settings-row__text">
              <p className="upd-settings-row__title">{t("updates.skipped")}</p>
              <p className="settings-hint">
                {t("updates.skippedHint", { version: settings.updateSkipped })}
              </p>
            </div>
            <button
              type="button"
              className="settings-button"
              onClick={() => {
                update({ updateSkipped: null });
              }}
            >
              {t("updates.stopSkipping")}
            </button>
          </div>
        )}
        <ChangelogRow />
      </Group>
    </Section>
  );
}

function ModeRow(props: {
  mode: UpdateMode;
  canInstall: boolean;
  onChange: (mode: UpdateMode) => void;
}) {
  const { t } = useTranslation("settings");
  const titleId = useId();
  const hintId = useId();
  const reasonId = useId();
  return (
    <div className="upd-settings-row">
      <div className="upd-settings-row__text">
        <p id={titleId} className="upd-settings-row__title">
          {t("updates.mode")}
        </p>
        <p id={hintId} className="settings-hint">
          {t("updates.modeHint")}
        </p>
        {!props.canInstall && (
          <p id={reasonId} className="settings-hint">
            {t("updates.autoUnavailable")}
          </p>
        )}
      </div>
      <div
        role="radiogroup"
        className="upd-segmented"
        aria-labelledby={titleId}
        aria-describedby={props.canInstall ? hintId : `${hintId} ${reasonId}`}
      >
        {UPDATE_MODES.map((mode) => (
          <label
            key={mode}
            className={`upd-segmented__option${props.mode === mode ? " upd-segmented__option--on" : ""}`}
          >
            <input
              type="radio"
              className="visually-hidden"
              name={titleId}
              value={mode}
              checked={props.mode === mode}
              disabled={mode === "auto" && !props.canInstall}
              onChange={() => {
                props.onChange(mode);
              }}
            />
            {t(`updates.${MODE_LABELS[mode]}`)}
          </label>
        ))}
      </div>
    </div>
  );
}

function IntervalRow(props: {
  hours: number;
  disabled: boolean;
  onChange: (hours: number) => void;
}) {
  const { t } = useTranslation("settings");
  const id = useId();
  const label = (hours: number) =>
    hours === 168 ? t("updates.intervalWeek") : t("updates.intervalHours", { count: hours });
  // A value set elsewhere (the range is 1–168) stays selectable instead of jumping to another.
  const choices: number[] = UPDATE_INTERVALS.some((hours) => hours === props.hours)
    ? [...UPDATE_INTERVALS]
    : [...UPDATE_INTERVALS, props.hours].sort((a, b) => a - b);
  return (
    <div className="upd-settings-row upd-settings-row--control">
      <div className="settings-control-row">
        <label htmlFor={id} className="upd-settings-row__title">
          {t("updates.interval")}
        </label>
        <Select
          id={id}
          value={String(props.hours)}
          disabled={props.disabled}
          options={choices.map((hours) => ({ value: String(hours), label: label(hours) }))}
          onChange={(value) => {
            props.onChange(Number(value));
          }}
        />
      </div>
      <p className="settings-hint">
        {props.hours === 168
          ? t("updates.intervalWeekHint")
          : t("updates.intervalHint", { count: props.hours })}
      </p>
    </div>
  );
}

function StatusRow({ status, off }: { status: UpdateStatus | null; off: boolean }) {
  const { t, i18n } = useTranslation("settings");
  const [failed, setFailed] = useState(false);
  // When the section opened; "today" only has to be right to the day.
  const [openedMs] = useState(Date.now);
  if (status === null) return <p className="settings-hint">{t("loading")}</p>;
  const busy = ["checking", "downloading", "verifying", "installing"].includes(status.phase);
  const version = status.offer?.version ?? "";
  // Offline is normal for a laptop; it is only mentioned here, never in a dialog.
  const offline = status.phase === "failed" && status.error === "network";
  const line = status.devBuild
    ? t("updates.devBuild")
    : off
      ? t("updates.state.off")
      : offline
        ? t("updates.state.offline")
        : t(`updates.state.${status.phase}`, { version });
  return (
    <div className="upd-settings-row">
      <div className="upd-settings-row__text" aria-live="polite">
        <p className={`upd-settings-row__title${offline ? " upd-settings-row__title--quiet" : ""}`}>
          {line}
        </p>
        {status.lastCheckMs !== null && (
          <p className="settings-hint">
            {lastChecked(
              status.lastCheckMs,
              Math.max(openedMs, status.lastCheckMs),
              i18n.language,
              t,
            )}
          </p>
        )}
        {failed && (
          <p role="alert" className="settings-error">
            {t("updates.checkFailed")}
          </p>
        )}
      </div>
      <div className="settings-actions">
        {status.offer !== null && status.phase !== "checking" && (
          <button type="button" className="settings-button" onClick={openUpdateDialog}>
            {t("updates.showUpdate")}
          </button>
        )}
        <button
          type="button"
          className="settings-button settings-button--primary"
          disabled={off || status.devBuild || busy}
          onClick={() => {
            setFailed(false);
            getApi()
              .updateCheckNow()
              .catch((error: unknown) => {
                console.error("Checking for updates failed", error);
                setFailed(true);
              });
          }}
        >
          {t("updates.checkNow")}
        </button>
      </div>
    </div>
  );
}

function ChangelogRow() {
  const { t } = useTranslation("settings");
  const [failed, setFailed] = useState(false);
  return (
    <div className="upd-settings-row">
      <div className="upd-settings-row__text">
        <p className="upd-settings-row__title">{t("updates.whatsNew")}</p>
        <p className="settings-hint">{t("updates.whatsNewHint")}</p>
        {failed && (
          <p role="alert" className="settings-error">
            {t("about.changelogFailed")}
          </p>
        )}
      </div>
      <button
        type="button"
        className="settings-link"
        onClick={() => {
          setFailed(false);
          getApi()
            .changelogOpen()
            .catch((error: unknown) => {
              console.error("Opening the changelog window failed", error);
              setFailed(true);
            });
        }}
      >
        {t("updates.openChangelog")}
      </button>
    </div>
  );
}
