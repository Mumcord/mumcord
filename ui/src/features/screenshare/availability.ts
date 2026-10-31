/**
 * Whether the user can start a screen share, and why not (FR-2.42, FR-2.46, FR-2.65). The reason
 * is shown as the share button's tooltip and read by screen readers.
 */
import { useCallback, useEffect, useState } from "react";
import type { TFunction } from "i18next";
import { getApi, type ShareStartBlock } from "../../api";
import { p2pBlockText } from "../p2p/relayReason";

export type ShareBlock =
  /** No voice server, or the own user isn't in a channel there yet. */
  | { kind: "notInChannel" }
  /** This system has no screen capture in Mumcord yet. */
  | { kind: "unsupportedPlatform" }
  /**
   * Nobody could watch: peer-to-peer is off on this server, or neither direct connections
   * (consent) nor the relay may be used (FR-4.3 – FR-4.5, AC-4.9). Reported by the engine.
   */
  | { kind: "p2p"; block: ShareStartBlock };

export interface ShareConditions {
  inChannel: boolean;
  /** The engine's block for the voice server, `null` when it allows sharing or isn't known yet. */
  p2p: ShareStartBlock | null;
  platform: SharePlatform | null;
}

/** The first reason that blocks sharing, or `null` when the user can share. */
export function shareBlock({ inChannel, p2p, platform }: ShareConditions): ShareBlock | null {
  if (!inChannel) return { kind: "notInChannel" };
  if (platform?.supported === false) return { kind: "unsupportedPlatform" };
  if (p2p !== null) return { kind: "p2p", block: p2p };
  return null;
}

/** The block as shown on the share button and read by screen readers. */
export function shareBlockText(
  t: TFunction<"screenshare">,
  tP2p: TFunction<"p2p">,
  block: ShareBlock,
): string {
  if (block.kind !== "p2p") return t(`blocked.${block.kind}`);
  return (
    p2pBlockText(tP2p, block.block.blocked, block.block.relayReason) ?? t("blocked.consentOff")
  );
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
 * What blocks starting a share on `serverId` (FR-4.3 – FR-4.5), `null` when nothing does or
 * until read. The engine sends no event when settings or the relay change, so `refresh` reads it
 * again, e.g. when the share button gets attention.
 */
export function useP2pShareBlock(serverId: number | null): {
  p2p: ShareStartBlock | null;
  refresh: () => Promise<ShareStartBlock | null>;
} {
  // Kept with its server, so a block read for another server never shows.
  const [read, setRead] = useState<{ serverId: number; block: ShareStartBlock | null } | null>(
    null,
  );
  const refresh = useCallback(() => {
    if (serverId === null) return Promise.resolve(null);
    return getApi()
      .screenShareBlock(serverId)
      .then(
        (block) => {
          setRead({ serverId, block });
          return block;
        },
        (error: unknown) => {
          console.error("Reading whether sharing is possible failed", error);
          return null;
        },
      );
  }, [serverId]);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  const p2p = read !== null && read.serverId === serverId ? read.block : null;
  return { p2p, refresh };
}
