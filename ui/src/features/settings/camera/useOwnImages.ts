import { useCallback, useEffect, useState } from "react";
import { getApi, type CameraBackgroundImage, type CameraBackgroundLibrary } from "../../../api";

/** The background library (FR-8.11, FR-8.15) with add and delete; `null` while loading. */
export interface OwnImages {
  library: CameraBackgroundLibrary | null;
  /** Adds the image at `path`; rejects with the engine error when it can't be used. */
  add: (path: string) => Promise<CameraBackgroundImage>;
  remove: (id: string) => Promise<void>;
}

const EMPTY: CameraBackgroundLibrary = { builtin: [], own: [] };

export function useOwnImages(): OwnImages {
  const [library, setLibrary] = useState<CameraBackgroundLibrary | null>(null);

  const reload = useCallback(async () => {
    try {
      setLibrary(await getApi().cameraBackgrounds());
    } catch (error) {
      console.error("Loading the camera backgrounds failed", error);
      setLibrary(EMPTY);
    }
  }, []);

  useEffect(() => {
    let active = true;
    getApi()
      .cameraBackgrounds()
      .then(
        (next) => {
          if (active) setLibrary(next);
        },
        (error: unknown) => {
          console.error("Loading the camera backgrounds failed", error);
          if (active) setLibrary(EMPTY);
        },
      );
    return () => {
      active = false;
    };
  }, []);

  const add = useCallback(
    async (path: string) => {
      const added = await getApi().cameraOwnBackgroundAdd(path);
      await reload();
      return added;
    },
    [reload],
  );

  const remove = useCallback(
    async (id: string) => {
      await getApi().cameraOwnBackgroundRemove(id);
      await reload();
    },
    [reload],
  );

  return { library, add, remove };
}

/**
 * The number the UI names an own image by. Only a random id is stored (no file name, so nothing
 * about the original file stays around), so images are numbered oldest first.
 */
export function ownImageNumber(images: readonly CameraBackgroundImage[], id: string): number {
  return images.findIndex((image) => image.id === id) + 1;
}
