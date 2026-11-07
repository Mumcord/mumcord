/**
 * The share start dialog (FR-7.35–7.37, mockup 01): quality picks before the system's window or
 * screen picker opens. It opens with the defaults of Settings → Screen sharing; "Use these
 * settings next time" stores the picks as the new defaults.
 */
import { useEffect, useEffectEvent, useId, useRef, useState, type ReactNode } from "react";
import type { TFunction } from "i18next";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { getApi, type Settings, type SharePreview } from "../../../api";
import { Select } from "../../../components/Select";
import { loadSoundBlock, SOUND_NEEDS_KEYS, type SoundBlock } from "../soundSupport";
import {
  CODECS,
  FPS,
  OPTIMIZE,
  RESOLUTIONS,
  UPLOAD_LIMITS,
  codecNote,
  codecOptionLabel,
  picksFromSettings,
  resolutionLabel,
  settingsWithPicks,
  viewersGetSummary,
  type StartPicks,
} from "./startPicks";
import { CopiesIcon, FanoutIcon, InfoIcon, ScreenIcon } from "./startIcons";
import { useSharePreviews } from "./useSharePreviews";
import "../../app/app.css";
import "../../settings/settings.css";
import "./shareStart.css";

/** Joins the codec note and hint as sentences: the note texts carry no closing full stop (#591). */
function sentences(lines: readonly (string | null)[]): string {
  return lines
    .filter((line) => line !== null)
    .map((line) => (/[.!?]$/.test(line) ? line : `${line}.`))
    .join(" ");
}

/** FR-7.35's defaults, used when the settings could not be read. */
const DEFAULT_PICKS: StartPicks = {
  choice: { resolution: "auto", fps: "30", optimize: "balanced", uploadLimit: "auto" },
  codec: "auto",
  sound: true,
};

export interface ShareStartDialogProps {
  serverId: number;
  onCancel: () => void;
  onConfirm: (picks: StartPicks) => void;
}

/** Loads the defaults first, so the dialog never flashes picks that change a moment later. */
export function ShareStartDialog(props: ShareStartDialogProps) {
  const [loaded, setLoaded] = useState<{
    settings: Settings | null;
    soundBlock: SoundBlock;
  } | null>(null);

  useEffect(() => {
    let active = true;
    void Promise.all([
      getApi()
        .settingsGet()
        .catch((error: unknown) => {
          console.error("Loading the screen share defaults failed", error);
          return null;
        }),
      loadSoundBlock(),
    ]).then(([settings, soundBlock]) => {
      if (active) setLoaded({ settings, soundBlock });
    });
    return () => {
      active = false;
    };
  }, []);

  if (loaded === null) return null;
  return <StartDialogContent {...props} {...loaded} />;
}

function StartDialogContent({
  serverId,
  onCancel,
  onConfirm,
  settings,
  soundBlock,
}: ShareStartDialogProps & { settings: Settings | null; soundBlock: SoundBlock }) {
  const { t } = useTranslation("screenshare");
  const titleId = useId();
  const introId = useId();
  const codecId = useId();
  const codecHintId = useId();
  const uploadId = useId();
  const soundId = useId();
  const soundNoteId = useId();
  const soundInfoId = useId();
  const ref = useRef<HTMLDialogElement>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);
  const [picks, setPicks] = useState<StartPicks>(() =>
    settings === null ? DEFAULT_PICKS : picksFromSettings(settings),
  );
  const [remember, setRemember] = useState(false);
  const previews = useSharePreviews(serverId, picks.choice);
  const preview = previews[picks.codec];
  const cancel = useEffectEvent(onCancel);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    // Enter starts at once: "Choose what to share" is the default button (FR-7.35).
    confirmRef.current?.focus();
    const onEscape = (event: Event) => {
      event.preventDefault();
      cancel();
    };
    dialog.addEventListener("cancel", onEscape);
    return () => {
      dialog.removeEventListener("cancel", onEscape);
      if (dialog.open) dialog.close();
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  const pick = (choice: Partial<StartPicks["choice"]>) => {
    setPicks((now) => ({ ...now, choice: { ...now.choice, ...choice } }));
  };

  const confirm = () => {
    const chosen = picks;
    if (remember && settings !== null) {
      getApi()
        .settingsUpdate(settingsWithPicks(settings, chosen))
        .catch((error: unknown) => {
          console.error("Saving the screen share defaults failed", error);
        });
    }
    onConfirm(chosen);
  };

  const note = preview === undefined ? null : codecNote(t, picks, preview);
  const hint = preview === undefined ? null : encoderHint(t, preview);

  // Rendered into <body>: the Apps menu sits inside the composer's <form>, and a nested form is
  // invalid HTML; WebKit then submitted the dialog's form natively and reloaded the page.
  return createPortal(
    <dialog
      ref={ref}
      className="app-dialog share-start"
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={introId}
    >
      <h2 id={titleId} className="app-dialog__title">
        {t("start.title")}
      </h2>
      <p id={introId} className="share-start__lead">
        {t("start.intro")}
      </p>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          // React bubbles the submit through the portal to the composer's form.
          event.stopPropagation();
          confirm();
        }}
      >
        <div className="share-start__field">
          <span className="share-start__label" aria-hidden="true">
            {t("start.resolution")}
          </span>
          <Segments
            label={t("start.resolution")}
            options={RESOLUTIONS.map((value) => ({ value, text: resolutionLabel(t, value) }))}
            value={picks.choice.resolution}
            onPick={(resolution) => {
              pick({ resolution });
            }}
          />
          <span className="share-start__hint">{t("start.resolutionAutoHint")}</span>
        </div>
        <div className="share-start__field">
          <span className="share-start__label" aria-hidden="true">
            {t("start.frameRate")}
          </span>
          <Segments
            label={t("start.frameRate")}
            options={FPS.map((value) => ({ value, text: t(`start.fps${value}` as const) }))}
            value={picks.choice.fps}
            onPick={(fps) => {
              pick({ fps });
            }}
          />
        </div>
        <fieldset className="share-start__field share-start__fieldset">
          <legend className="share-start__label">{t("start.optimize")}</legend>
          <div className="share-start__modes">
            {OPTIMIZE.map((value) => {
              const key = MODE_KEYS[value];
              return (
                <label
                  key={value}
                  className="share-start__mode"
                  data-picked={picks.choice.optimize === value ? "" : undefined}
                >
                  <input
                    type="radio"
                    name="share-start-optimize"
                    checked={picks.choice.optimize === value}
                    onChange={() => {
                      pick({ optimize: value });
                    }}
                  />
                  <strong>{t(`start.${key}` as const)}</strong>
                  <span className="share-start__mode-hint">{t(`start.${key}Hint` as const)}</span>
                </label>
              );
            })}
          </div>
        </fieldset>
        <div className="share-start__row2">
          <div className="share-start__field">
            <label className="share-start__label" htmlFor={codecId}>
              {t("start.codec")}
            </label>
            <Select
              id={codecId}
              className="share-start__select"
              value={picks.codec}
              describedBy={note !== null || hint !== null ? codecHintId : undefined}
              options={CODECS.map((value) => ({
                value,
                label: codecOptionLabel(t, value, previews[value]),
              }))}
              onChange={(next) => {
                const codec = CODECS.find((value) => value === next);
                if (codec !== undefined) setPicks((now) => ({ ...now, codec }));
              }}
            />
            {(note !== null || hint !== null) && (
              <span id={codecHintId} className="share-start__hint share-start__note">
                <InfoIcon />
                <span>{sentences([note, hint])}</span>
              </span>
            )}
          </div>
          <div className="share-start__field">
            <label className="share-start__label" htmlFor={uploadId}>
              {t("start.uploadLimit")}
            </label>
            <Select
              id={uploadId}
              className="share-start__select"
              value={picks.choice.uploadLimit}
              options={UPLOAD_LIMITS.map((value) => ({
                value,
                label:
                  value === "auto" ? t("start.uploadAuto") : t(`start.upload${value}` as const),
              }))}
              onChange={(next) => {
                const uploadLimit = UPLOAD_LIMITS.find((value) => value === next);
                if (uploadLimit !== undefined) pick({ uploadLimit });
              }}
            />
          </div>
        </div>
        <div className="settings-switch share-start__switch">
          <div className="settings-switch__text">
            <label htmlFor={soundId}>{t("start.sound")}</label>
            {soundBlock !== null && (
              <p id={soundNoteId} className="share-start__soundnote">
                {t(`start.${SOUND_NEEDS_KEYS[soundBlock]}`)}
              </p>
            )}
          </div>
          <input
            id={soundId}
            type="checkbox"
            role="switch"
            className="settings-switch__input"
            checked={soundBlock === null && picks.sound}
            disabled={soundBlock !== null}
            aria-describedby={soundBlock !== null ? `${soundInfoId} ${soundNoteId}` : soundInfoId}
            onChange={(event) => {
              const sound = event.target.checked;
              setPicks((now) => ({ ...now, sound }));
            }}
          />
        </div>
        <p id={soundInfoId} className="share-start__infoline">
          <InfoIcon />
          {t("start.soundInfo")}
        </p>
        <ViewersGet preview={preview} />
        <label className="app-dialog__remember">
          <input
            type="checkbox"
            checked={remember}
            onChange={(event) => {
              setRemember(event.target.checked);
            }}
          />
          {t("start.remember")}
        </label>
        <div className="app-dialog__actions">
          <button type="button" className="button" onClick={onCancel}>
            {t("start.cancel")}
          </button>
          <button ref={confirmRef} type="submit" className="button button--primary">
            {t("start.confirm")}
          </button>
        </div>
      </form>
    </dialog>,
    document.body,
  );
}

/** What the encoder costs: the processor's cores it takes, or almost nothing on the graphics card. */
function encoderHint(t: TFunction<"screenshare">, preview: SharePreview) {
  if (preview.encoder.gpu !== null) return t("start.codecHintGpu");
  if (preview.cpuCores === null) return null;
  return t("start.codecHintCpu", preview.cpuCores);
}

const MODE_KEYS = {
  balanced: "optimizeBalanced",
  text: "optimizeSharp",
  motion: "optimizeSmooth",
} as const;

/** A row of toggle buttons where exactly one is pressed, as in the mockup. */
function Segments<T extends string>({
  label,
  options,
  value,
  onPick,
}: {
  label: string;
  options: { value: T; text: string }[];
  value: T;
  onPick: (value: T) => void;
}) {
  return (
    <span className="share-start__seg" role="group" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          className="share-start__seg-button"
          aria-pressed={option.value === value}
          onClick={() => {
            onPick(option.value);
          }}
        >
          {option.text}
        </button>
      ))}
    </span>
  );
}

/** "Viewers get" (FR-7.36): updates live and is read out politely when it changes. */
function ViewersGet({ preview }: { preview: SharePreview | undefined }) {
  const { t } = useTranslation("screenshare");
  let body: ReactNode = null;
  if (preview !== undefined) {
    const broadcast = preview.path.kind === "broadcast";
    const path = broadcast
      ? t("start.pathRelay", { count: preview.path.maxViewers })
      : t("start.pathDirect", { count: preview.path.maxViewers });
    body = (
      <div>
        <p className="share-start__gets-main">
          <strong>{t("start.viewersGet")}</strong> {viewersGetSummary(t, preview)}
        </p>
        <p className="share-start__gets-sub" data-path={preview.path.kind}>
          {broadcast ? <FanoutIcon /> : <CopiesIcon />}
          <span>
            {path}
            {preview.encoder.codec === "av1" && ` ${t("start.olderMumcord")}`}
          </span>
        </p>
      </div>
    );
  }
  return (
    <div className="share-start__gets" role="status">
      {body !== null && (
        <>
          <span className="share-start__gets-icon">
            <ScreenIcon />
          </span>
          {body}
        </>
      )}
    </div>
  );
}
