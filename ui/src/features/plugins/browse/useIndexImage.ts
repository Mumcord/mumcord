import { useEffect, useState } from "react";
import { getApi, type PluginIndexImage } from "../../../api";

/** Loaded images by plugin and image; `null` marks one that could not be loaded. */
const loaded = new Map<string, string | null>();

function keyOf(id: string, image: PluginIndexImage): string {
  return image.kind === "icon" ? `${id}:icon` : `${id}:screenshot:${String(image.index)}`;
}

/**
 * A store image as a data URL from the engine (the UI never fetches from the network), or
 * `null` while loading, when the plugin has none or when it failed.
 */
export function useIndexImage(id: string, image: PluginIndexImage | null): string | null {
  const key = image === null ? null : keyOf(id, image);
  // Bumped when an image arrived, so the component reads the module cache again.
  const [, setLoads] = useState(0);

  useEffect(() => {
    if (image === null || key === null || loaded.has(key)) return;
    let cancelled = false;
    getApi()
      .pluginIndexImage(id, image)
      .then((dataUrl) => {
        // Only data URLs of raster images are accepted, never a remote address.
        const safe = /^data:image\/(png|jpeg|webp|gif);base64,/.test(dataUrl) ? dataUrl : null;
        loaded.set(key, safe);
        if (!cancelled) setLoads((n) => n + 1);
      })
      .catch((error: unknown) => {
        console.warn("Loading a plugin image failed", error);
        loaded.set(key, null);
      });
    return () => {
      cancelled = true;
    };
    // `image` is identified by `key`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, key]);

  return key === null ? null : (loaded.get(key) ?? null);
}

/** Tests only. */
export function resetIndexImages(): void {
  loaded.clear();
}
