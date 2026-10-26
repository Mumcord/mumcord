/**
 * Whether the user can start a screen share, and why not (FR-2.42, FR-2.46, FR-2.65). The reason
 * is shown as the share button's tooltip and read by screen readers.
 */
import { useCallback, useEffect, useState } from "react";
import { getApi, type Consent } from "../../api";

export type ShareBlock =
  /** No voice server, or the own user isn't in a channel there yet. */
  | "notInChannel"
  /** The user said no to direct connections, so nobody could watch. */
  | "consentOff"
  /** This system has no screen capture in Mumcord yet. */
  | "unsupportedPlatform";

export interface ShareConditions {
  inChannel: boolean;
  consent: Consent | null;
  platform: SharePlatform | null;
}

/** The first reason that blocks sharing, or `null` when the user can share. */
export function shareBlock({ inChannel, consent, platform }: ShareConditions): ShareBlock | null {
  if (!inChannel) return "notInChannel";
  if (platform?.supported === false) return "unsupportedPlatform";
  if (consent === "no") return "consentOff";
  return null;
}

export interface SharePlatform {
  /** Mumcord can capture the screen here. */
  supported: boolean;
  /** The shared app's sound can be sent (FR-2.42); Linux never sends sound. */
  sound: boolean;
}

/**
 * What this OS supports, from the backend's OS description ("linux x86_64"). macOS sends the
 * shared app's sound from macOS 13 on; older versions are below the supported range (current and
 * previous major), and the engine sends no sound there. Windows sends the picked app's sound from
 * Windows 10 2004 on, for a window of a single app; the engine leaves it out otherwise.
 */
export function platformOf(os: string): SharePlatform {
  const name = os.toLowerCase();
  if (name.startsWith("macos")) return { supported: true, sound: true };
  if (name.startsWith("windows")) return { supported: true, sound: true };
  return { supported: name.startsWith("linux"), sound: false };
}

/** The platform support, `null` until the app info arrived. */
export function useSharePlatform(): SharePlatform | null {
  const [platform, setPlatform] = useState<SharePlatform | null>(null);
  useEffect(() => {
    let active = true;
    getApi()
      .appInfo()
      .then(
        (info) => {
          if (active) setPlatform(platformOf(info.os));
        },
        (error: unknown) => {
          console.error("Reading the app info failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);
  return platform;
}

/**
 * The direct-connection consent (FR-2.36), `null` until read. The engine sends no event when
 * settings change, so `refresh` reads it again, e.g. when the share button gets attention.
 */
export function useDirectConsent(): {
  consent: Consent | null;
  refresh: () => Promise<Consent | null>;
} {
  const [consent, setConsent] = useState<Consent | null>(null);
  const refresh = useCallback(
    () =>
      getApi()
        .settingsGet()
        .then(
          (settings) => {
            setConsent(settings.allowDirectConnections);
            return settings.allowDirectConnections;
          },
          (error: unknown) => {
            console.error("Reading the settings failed", error);
            return null;
          },
        ),
    [],
  );
  useEffect(() => {
    void refresh();
  }, [refresh]);
  return { consent, refresh };
}
