/**
 * Plugin images reach the UI only through `plugin_asset` (the UI never reads plugin files) and
 * are shown via blob URLs (FR-6.42). URLs are shared and reference-counted so a list of 50
 * identical icons loads the file once, and revoked when nothing shows them any more.
 */
import { useEffect, useState } from "react";
import { getApi } from "../../../api";

interface Entry {
  url: Promise<string>;
  users: number;
}

const cache = new Map<string, Entry>();

function decode(base64: string): Uint8Array<ArrayBuffer> {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

function acquire(key: string, plugin: string, path: string): Promise<string> {
  let entry = cache.get(key);
  if (entry === undefined) {
    const url = getApi()
      .pluginAsset(plugin, path)
      .then(({ mimeType, base64 }) =>
        URL.createObjectURL(new Blob([decode(base64)], { type: mimeType })),
      );
    entry = { url, users: 0 };
    cache.set(key, entry);
  }
  entry.users++;
  return entry.url;
}

function release(key: string): void {
  const entry = cache.get(key);
  if (entry === undefined) return;
  entry.users--;
  if (entry.users > 0) return;
  cache.delete(key);
  entry.url.then(
    (url) => {
      URL.revokeObjectURL(url);
    },
    () => undefined,
  );
}

/**
 * The blob URL of a plugin asset: `undefined` while loading, `null` if it cannot be shown
 * (the caller then shows the alt text).
 */
export function usePluginAsset(plugin: string, path: string | null): string | null | undefined {
  const key = `${plugin}\n${path ?? ""}`;
  const [loaded, setLoaded] = useState<{ key: string; url: string | null }>();
  useEffect(() => {
    if (path === null) return;
    let active = true;
    acquire(key, plugin, path).then(
      (url) => {
        if (active) setLoaded({ key, url });
      },
      (error: unknown) => {
        console.error("Loading a plugin image failed", error);
        if (active) setLoaded({ key, url: null });
      },
    );
    return () => {
      active = false;
      release(key);
    };
  }, [key, plugin, path]);
  if (path === null) return null;
  return loaded?.key === key ? loaded.url : undefined;
}

/** The blob URL of an installed plugin's manifest icon, or `null` while loading or without one. */
export function usePluginIconUrl(plugin: string, icon: string | null | undefined): string | null {
  return usePluginAsset(plugin, icon ?? null) ?? null;
}
