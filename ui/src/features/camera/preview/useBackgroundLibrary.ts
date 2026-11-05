/** The background image library of the preview (FR-8.11, FR-8.14): thumbnails and Add image. */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  EngineError,
  getApi,
  type CameraBackgroundImage,
  type CameraBackgroundLibrary,
} from "../../../api";
import { pickOpenPath } from "../../settings/fileDialogs";

const IMAGE_FILTER = { name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] };

export interface BackgroundLibrary {
  library: CameraBackgroundLibrary;
  adding: boolean;
  /** The last Add image failed: "This image can't be used …". */
  addFailed: boolean;
  /** Opens the file picker and imports the image; `null` when cancelled or refused. */
  add: () => Promise<CameraBackgroundImage | null>;
}

const EMPTY: CameraBackgroundLibrary = { builtin: [], own: [] };

export function useBackgroundLibrary(): BackgroundLibrary {
  const { t } = useTranslation("camera");
  const [library, setLibrary] = useState<CameraBackgroundLibrary>(EMPTY);
  const [adding, setAdding] = useState(false);
  const [addFailed, setAddFailed] = useState(false);

  useEffect(() => {
    let active = true;
    getApi()
      .cameraBackgrounds()
      .then(
        (next) => {
          if (active) setLibrary(next);
        },
        (error: unknown) => {
          console.error("Loading the background images failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);

  const add = async (): Promise<CameraBackgroundImage | null> => {
    setAddFailed(false);
    const path = await pickOpenPath(IMAGE_FILTER, t("preview.addImageLabel"));
    if (path === null) return null;
    setAdding(true);
    try {
      const image = await getApi().cameraOwnBackgroundAdd(path);
      setLibrary((now) => ({ ...now, own: [...now.own, image] }));
      return image;
    } catch (error: unknown) {
      // The reason is always the same text for the user; the path itself is never logged.
      console.error(
        "Adding a background image failed",
        error instanceof EngineError ? error.code : "unexpected error",
      );
      setAddFailed(true);
      return null;
    } finally {
      setAdding(false);
    }
  };

  return { library, adding, addFailed, add };
}
