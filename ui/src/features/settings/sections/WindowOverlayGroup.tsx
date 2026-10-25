import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type OverlaySettings, type OverlayState } from "../../../api";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import { CornerSelect, SliderField } from "./OverlayFields";

/** Window overlay (FR-1.44 – FR-1.46): the transparent always-on-top window. */
export function WindowOverlayGroup() {
  const { t } = useTranslation(["settings", "overlay"]);
  const [state, setState] = useState<OverlayState | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let active = true;
    const api = getApi();
    api.overlayState().then(
      (loaded) => {
        if (active) setState(loaded);
      },
      (e: unknown) => {
        console.error("Reading the overlay state failed", e);
        if (active) setFailed(true);
      },
    );
    const unlisten = api.on("overlayChanged", (event) => {
      setState(event.state);
    });
    return () => {
      active = false;
      unlisten();
    };
  }, []);

  const run = (call: Promise<OverlayState>) => {
    call.then(setState, (e: unknown) => {
      console.error("Changing the overlay failed", e);
      setFailed(true);
    });
  };
  const change = (patch: Partial<OverlaySettings>) => {
    if (state === null) return;
    setFailed(false);
    run(getApi().overlayUpdate({ ...state.settings, ...patch }));
  };

  if (state === null) {
    return (
      <Group title={t("settings:overlay.window.title")}>
        <p className="settings-hint" role={failed ? "alert" : undefined}>
          {failed ? t("settings:overlay.window.loadFailed") : t("settings:loading")}
        </p>
      </Group>
    );
  }

  const s = state.settings;
  return (
    <Group title={t("settings:overlay.window.title")}>
      <Switch
        label={t("settings:overlay.window.enable")}
        description={t("settings:overlay.window.enableHint")}
        checked={s.enabled}
        onChange={(enabled) => {
          setFailed(false);
          run(getApi().overlaySetEnabled(enabled));
        }}
      />
      {!state.onTopSupported && state.explanationKey !== null && (
        <p className="settings-hint" role="note">
          {t(state.explanationKey, { ns: "overlay" })}
        </p>
      )}
      {!state.positionSupported && (
        <p className="settings-hint" role="note">
          {t("settings:overlay.window.positionUnsupported")}
        </p>
      )}
      <p className="settings-hint">{t("overlay:limits.exclusiveFullscreen")}</p>
      <CornerSelect
        label={t("settings:overlay.corner.label")}
        value={s.corner}
        custom={s.customPosition !== null}
        disabled={!state.positionSupported}
        onChange={(corner) => {
          change({ corner, customPosition: null });
        }}
      />
      <div className="settings-field">
        <button
          type="button"
          className="settings-button"
          disabled={!s.enabled || !state.positionSupported}
          aria-pressed={state.moveMode}
          onClick={() => {
            setFailed(false);
            run(getApi().overlayMoveMode(!state.moveMode));
          }}
        >
          {state.moveMode
            ? t("settings:overlay.window.moveDone")
            : t("settings:overlay.window.move")}
        </button>
      </div>
      <SliderField
        label={t("settings:overlay.window.size")}
        value={s.width}
        min={160}
        max={800}
        step={10}
        unit=" px"
        onChange={(width) => {
          change({ width });
        }}
      />
      <SliderField
        label={t("settings:overlay.opacity")}
        value={s.opacity}
        min={20}
        max={100}
        step={5}
        unit="%"
        onChange={(opacity) => {
          change({ opacity });
        }}
      />
      <Switch
        label={t("settings:overlay.window.wholeChannel")}
        description={t("settings:overlay.window.wholeChannelHint")}
        checked={s.showWholeChannel}
        onChange={(showWholeChannel) => {
          change({ showWholeChannel });
        }}
      />
      <Switch
        label={t("settings:overlay.window.channelName")}
        checked={s.showChannelName}
        onChange={(showChannelName) => {
          change({ showChannelName });
        }}
      />
      {failed && (
        <p className="settings-hint" role="alert">
          {t("settings:overlay.window.saveFailed")}
        </p>
      )}
    </Group>
  );
}
