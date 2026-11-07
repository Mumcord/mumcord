/**
 * The camera preview dialog (FR-8.10–8.14, mockups 03–04): turning the camera on always starts
 * here. The local camera runs but nothing is sent until **Turn on camera**.
 */
import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Trans, useTranslation } from "react-i18next";
import { getApi, type CameraBackground, type CameraDevice, type Settings } from "../../../api";
import { selfUser, useServerStateOf } from "../../channels/serverStore";
import { useAppOs } from "../../screenshare/useAppOs";
import { BackgroundPicker } from "./BackgroundPicker";
import { saveCameraSettings } from "./cameraSettings";
import { DevicePicker } from "./DevicePicker";
import { CameraIcon, CheckIcon, LoadIcon } from "./previewIcons";
import {
  canRetryAccess,
  desktopOs,
  failureText,
  loadLine,
  loadLineText,
  refusalHint,
} from "./previewLogic";
import { RefusalAction } from "../../p2p/RefusalAction";
import { PreviewPicture } from "./PreviewPicture";
import { applyLiveBackground, setLiveBackground } from "./liveBackground";
import { useEffectsLoad } from "./useEffectsLoad";
import { useBackgroundLibrary } from "./useBackgroundLibrary";
import { useCameraDevices } from "./useCameraDevices";
import { usePreviewSession } from "./usePreviewSession";
import "../../app/app.css";
import "../../settings/settings.css";
import "./preview.css";

export interface CameraPreviewDialogProps {
  serverId: number;
  onClose: () => void;
}

interface Loaded {
  settings: Settings | null;
  devices: CameraDevice[];
}

/** Loads the defaults first, so the dialog never flashes a background that changes a moment later. */
export function CameraPreviewDialog(props: CameraPreviewDialogProps) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  useEffect(() => {
    let active = true;
    const api = getApi();
    void Promise.all([
      api.settingsGet().catch((error: unknown) => {
        console.error("Loading the camera defaults failed", error);
        return null;
      }),
      api.cameraDevices().catch((error: unknown) => {
        console.error("Listing the cameras failed", error);
        return [];
      }),
    ]).then(([settings, devices]) => {
      if (active) setLoaded({ settings, devices });
    });
    return () => {
      active = false;
    };
  }, []);
  if (loaded === null) return null;
  return <DialogContent {...props} {...loaded} />;
}

function DialogContent({
  serverId,
  onClose,
  settings,
  devices,
}: CameraPreviewDialogProps & Loaded) {
  const { t } = useTranslation("camera");
  const titleId = useId();
  const noteId = useId();
  const statusId = useId();
  const ref = useRef<HTMLDialogElement>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);
  const os = desktopOs(useAppOs());
  const session = usePreviewSession(serverId, onClose);
  const effects = useEffectsLoad();
  const own = useBackgroundLibrary();
  const cameras = useCameraDevices(devices);
  const channel = useOwnChannelName(serverId);
  const [background, setBackground] = useState<CameraBackground>(
    settings?.cameraDefaultBackground ?? { kind: "blur" },
  );
  const [blurStrength, setBlurStrength] = useState(settings?.cameraBlurStrength ?? 50);
  const [remember, setRemember] = useState(true);
  const [deviceFailure, setDeviceFailure] = useState<string | null>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    dialog.showModal();
    // Keeps the browser from focusing the first control (the camera picker) instead.
    dialog.focus();
    // Escape is Cancel: the camera goes off with the dialog (FR-8.12).
    const onEscape = (event: Event) => {
      event.preventDefault();
      session.cancel();
    };
    dialog.addEventListener("cancel", onEscape);
    return () => {
      dialog.removeEventListener("cancel", onEscape);
      if (dialog.open) dialog.close();
    };
    // `session.cancel` is the stable `onClose` of the host.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Turn on camera is the default button, but it can only take the focus once the preview runs.
  const preview = session.state === "preview";
  const focusedOnce = useRef(false);
  useEffect(() => {
    if (!preview || focusedOnce.current) return;
    focusedOnce.current = true;
    const active = document.activeElement;
    if (active === null || active === document.body || active === ref.current) {
      confirmRef.current?.focus();
    }
  }, [preview]);

  const applyBackground = (next: CameraBackground, strength: number) => {
    setBackground(next);
    setBlurStrength(strength);
    applyLiveBackground({ background: next, blurStrength: strength });
  };

  const turnOn = () => {
    if (remember) {
      saveCameraSettings({
        cameraDefaultBackground: background,
        cameraBlurStrength: blurStrength,
      }).catch((error: unknown) => {
        console.error("Saving the default camera background failed", error);
      });
    }
    // The room bar's picker starts from what the camera goes on air with.
    setLiveBackground({ background, blurStrength });
    session.turnOn();
  };

  const requesting = session.state === "requesting";
  const running = session.state === "preview" || requesting;
  const line = loadLine(effects);
  // An empty device list already says "No camera found" at the picker.
  const failure = session.failure === "noCamera" && cameras.length === 0 ? null : session.failure;
  const hint = failure === null ? null : refusalHint(failure);

  return createPortal(
    <dialog
      ref={ref}
      className="app-dialog cam-dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={noteId}
      aria-busy={requesting}
      tabIndex={-1}
    >
      <h2 id={titleId} className="app-dialog__title">
        {t("preview.title")}
      </h2>
      <div className="cam-preview">
        <div>
          <PreviewPicture mirror={settings?.cameraMirrorSelf ?? true} live={running} />
          <p id={noteId} className="cam-preview__note">
            <CheckIcon size={14} />
            <span>
              <Trans
                ns="camera"
                i18nKey="preview.privacyNote"
                components={{ channel: <strong>{channel}</strong> }}
              />
            </span>
          </p>
        </div>
        <div>
          <DevicePicker
            devices={cameras}
            initialId={settings?.cameraDeviceId ?? null}
            os={os}
            disabled={requesting}
            onFailure={setDeviceFailure}
          />
          <BackgroundPicker
            background={background}
            blurStrength={blurStrength}
            library={own.library}
            effectsOff={effects?.unavailable != null}
            adding={own.adding}
            addError={own.addFailed ? t("imageImportFailed") : null}
            onBackground={(next) => {
              applyBackground(next, blurStrength);
            }}
            onBlurStrength={(strength) => {
              applyBackground(background, strength);
            }}
            onAddImage={() => {
              void own.add().then((added) => {
                if (added !== null) applyBackground({ kind: "own", id: added.id }, blurStrength);
              });
            }}
          />
          {line !== null && (
            <p className="cam-load" data-busy={line.kind === "tooBusy" ? "" : undefined}>
              <LoadIcon />
              {loadLineText(t, line)}
            </p>
          )}
        </div>
      </div>
      <label className="app-dialog__remember">
        <input
          type="checkbox"
          checked={remember}
          onChange={(event) => {
            setRemember(event.target.checked);
          }}
        />{" "}
        {t("preview.remember")}
      </label>
      <div id={statusId} className="cam-dialog__status" role="status" aria-live="polite">
        {requesting && t("preview.starting")}
        {failure !== null && (
          <span className="app-dialog__error">
            {failureText(t, failure, os)}{" "}
            {canRetryAccess(failure, os) && (
              <button type="button" className="button button--link" onClick={session.retry}>
                {t("privacy.linuxAction")}
              </button>
            )}
            {hint !== null && <RefusalAction hint={hint} serverId={serverId} />}
          </span>
        )}
        {deviceFailure !== null && <span className="app-dialog__error">{deviceFailure}</span>}
      </div>
      <div className="app-dialog__actions">
        <button type="button" className="button" onClick={session.cancel}>
          {t("preview.cancel")}
        </button>
        <button
          ref={confirmRef}
          type="button"
          className="button button--primary"
          disabled={session.state !== "preview"}
          aria-describedby={statusId}
          onClick={turnOn}
        >
          <CameraIcon /> {requesting ? t("preview.starting") : t("preview.turnOn")}
        </button>
      </div>
    </dialog>,
    document.body,
  );
}

function useOwnChannelName(serverId: number): string {
  const state = useServerStateOf(serverId);
  const self = selfUser(state);
  return (self === undefined ? undefined : state.channels.get(self.channelId)?.name) ?? "";
}
