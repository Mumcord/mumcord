import { useCallback, useEffect, useState } from "react";
import { getApi, type OwnBackground } from "../../../api";

/** The user's own background images (FR-8.15) with add and delete; `null` while loading. */
export interface OwnImages {
  images: readonly OwnBackground[] | null;
  /** Adds the image at `path`; rejects with the engine error when it can't be used. */
  add: (path: string) => Promise<OwnBackground>;
  remove: (id: string) => Promise<void>;
}

export function useOwnImages(): OwnImages {
  const [images, setImages] = useState<readonly OwnBackground[] | null>(null);

  const reload = useCallback(async () => {
    try {
      setImages(await getApi().cameraOwnBackgrounds());
    } catch (error) {
      console.error("Loading the own background images failed", error);
      setImages([]);
    }
  }, []);

  useEffect(() => {
    let active = true;
    getApi()
      .cameraOwnBackgrounds()
      .then(
        (list) => {
          if (active) setImages(list);
        },
        (error: unknown) => {
          console.error("Loading the own background images failed", error);
          if (active) setImages([]);
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

  return { images, add, remove };
}

/**
 * The name the UI shows for an own image. Only a random id is stored (no file name, so nothing
 * about the original file stays around), so images are numbered in the order they were added.
 */
export function ownImageNumber(images: readonly OwnBackground[], id: string): number {
  return images.findIndex((image) => image.id === id) + 1;
}
