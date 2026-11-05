/** Background None / Blur / Image with blur strength and the image library (FR-8.11, mockups 03–04). */
import { useId, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { CameraBackground, CameraBackgroundLibrary } from "../../../api";
import { BlurIcon, CheckIcon, ImageIcon, LockIcon, NoneIcon, UploadIcon } from "./previewIcons";
import {
  BUILTIN_BACKGROUNDS,
  backgroundForMode,
  backgroundMode,
  sameBackground,
  type BackgroundMode,
} from "./previewLogic";

export interface BackgroundPickerProps {
  background: CameraBackground;
  blurStrength: number;
  library: CameraBackgroundLibrary;
  /** Background effects can't run on this PC (FR-8.33): only None is offered. */
  effectsOff: boolean;
  adding: boolean;
  addError: string | null;
  onBackground: (background: CameraBackground) => void;
  onBlurStrength: (strength: number) => void;
  onAddImage: () => void;
}

const MODES: readonly { mode: BackgroundMode; icon: ReactNode; key: string }[] = [
  { mode: "none", icon: <NoneIcon />, key: "preview.backgroundNone" },
  { mode: "blur", icon: <BlurIcon />, key: "preview.backgroundBlur" },
  { mode: "image", icon: <ImageIcon />, key: "preview.backgroundImage" },
];

export function BackgroundPicker(props: BackgroundPickerProps) {
  const { t } = useTranslation("camera");
  const name = useId();
  const labelId = useId();
  const mode = backgroundMode(props.background);
  const [lastImage, setLastImage] = useState<CameraBackground | null>(
    mode === "image" ? props.background : null,
  );

  const pickImage = (background: CameraBackground) => {
    setLastImage(background);
    props.onBackground(background);
  };

  return (
    <>
      <div className="cam-field">
        <span className="cam-field__label" id={labelId}>
          {t("preview.background")}
        </span>
        <div className="bg-modes" role="radiogroup" aria-labelledby={labelId}>
          {MODES.map((item) => (
            <label
              key={item.mode}
              className="bg-mode"
              data-checked={mode === item.mode ? "" : undefined}
            >
              <input
                type="radio"
                className="cam-sr-only"
                name={name}
                checked={mode === item.mode}
                disabled={props.effectsOff && item.mode !== "none"}
                onChange={() => {
                  props.onBackground(backgroundForMode(item.mode, lastImage));
                }}
              />
              {item.icon}
              {t(item.key)}
            </label>
          ))}
        </div>
      </div>
      {mode === "blur" && (
        <BlurStrength value={props.blurStrength} onChange={props.onBlurStrength} />
      )}
      {mode === "image" && <ImageLibrary {...props} onPick={pickImage} />}
    </>
  );
}

function BlurStrength({ value, onChange }: { value: number; onChange: (value: number) => void }) {
  const { t } = useTranslation("camera");
  const id = useId();
  return (
    <div className="cam-field">
      <label className="cam-field__label" htmlFor={id}>
        {t("preview.blurStrength")}
      </label>
      <input
        id={id}
        className="cam-strength"
        type="range"
        min={0}
        max={100}
        step={1}
        value={value}
        aria-valuetext={t("preview.blurStrengthValue", { value })}
        onChange={(event) => {
          onChange(Number(event.target.value));
        }}
      />
      <span className="cam-strength__ends" aria-hidden="true">
        <span>{t("preview.blurLight")}</span>
        <span>{t("preview.blurStrong")}</span>
      </span>
    </div>
  );
}

function ImageLibrary({
  background,
  library,
  adding,
  addError,
  onAddImage,
  onPick,
}: BackgroundPickerProps & { onPick: (background: CameraBackground) => void }) {
  const { t } = useTranslation("camera");
  const name = useId();
  const labelId = useId();
  const errorId = useId();
  const thumb = (
    key: string,
    picked: CameraBackground,
    label: string,
    src: string | null,
    own: boolean,
  ) => {
    const checked = sameBackground(background, picked);
    return (
      <label key={key} className="bg-thumb" data-checked={checked ? "" : undefined}>
        <input
          type="radio"
          className="cam-sr-only"
          name={name}
          checked={checked}
          aria-label={label}
          onChange={() => {
            onPick(picked);
          }}
        />
        {src !== null && <img src={src} alt="" draggable={false} />}
        {own && (
          <span className="bg-thumb__own" aria-hidden="true">
            {t("preview.imageYours")}
          </span>
        )}
        {checked && (
          <span className="bg-thumb__check" aria-hidden="true">
            <CheckIcon size={10} />
          </span>
        )}
      </label>
    );
  };

  return (
    <div className="cam-field">
      <span className="cam-field__label" id={labelId}>
        {t("preview.backgroundImage")}
      </span>
      <div className="bg-images" role="radiogroup" aria-labelledby={labelId}>
        {BUILTIN_BACKGROUNDS.map((item) =>
          thumb(
            item.id,
            { kind: "builtin", id: item.id },
            t(`backgrounds.${item.key}`),
            library.builtin.find((image) => image.id === item.id)?.thumbnail ?? null,
            false,
          ),
        )}
        {library.own.map((image, index) =>
          thumb(
            `own-${image.id}`,
            { kind: "own", id: image.id },
            t("preview.imageYoursNumbered", { number: index + 1 }),
            image.thumbnail,
            true,
          ),
        )}
        <button
          type="button"
          className="bg-thumb bg-thumb--add"
          aria-label={t("preview.addImageLabel")}
          aria-describedby={addError === null ? undefined : errorId}
          aria-busy={adding}
          disabled={adding}
          onClick={onAddImage}
        >
          <span>
            <UploadIcon />
            {t("preview.addImage")}
          </span>
        </button>
      </div>
      {addError !== null && (
        <p id={errorId} className="cam-preview__error" role="alert">
          {addError}
        </p>
      )}
      <p className="cam-hint">
        <LockIcon size={11} /> {t("preview.imagesHint")}
      </p>
    </div>
  );
}
