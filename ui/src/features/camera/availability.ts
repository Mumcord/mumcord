/**
 * Whether the user can turn their camera on (FR-8.2, FR-8.4, FR-8.16) and where the own camera
 * is (FR-8.17). Availability comes per server from `cameraAvailabilityChanged`, read once with
 * `cameraUnavailable` after each sync; the own camera from `cameraChanged`.
 */
import { useEffect, useSyncExternalStore } from "react";
import {
  getApi,
  type CameraUnavailable,
  type CameraView,
  type MumcordApi,
  type Unsubscribe,
} from "../../api";

/** Why the camera can't be turned on: not in a voice channel there, or an FR-8.4 reason. */
export type CameraBlock = "notInChannel" | CameraUnavailable;

// A server missing from the map has not been read yet; it counts as available until it is.
let unavailable = new Map<number, CameraUnavailable | null>();
let view: CameraView | null = null;
const requested = new Set<number>();
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

function setUnavailable(serverId: number, reason: CameraUnavailable | null | undefined): void {
  unavailable = new Map(unavailable);
  if (reason === undefined) unavailable.delete(serverId);
  else unavailable.set(serverId, reason);
  notify();
}

function read(api: MumcordApi, serverId: number): void {
  api.cameraUnavailable(serverId).then(
    (reason) => {
      // An event that arrived meanwhile is newer than this answer.
      if (boundApi === api && !unavailable.has(serverId)) setUnavailable(serverId, reason);
    },
    (error: unknown) => {
      console.error("Reading the camera availability failed", error);
    },
  );
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  unavailable = new Map();
  requested.clear();
  view = null;
  unsubscribers = [
    api.on("cameraAvailabilityChanged", ({ serverId, reason }) => {
      setUnavailable(serverId, reason);
    }),
    api.on("cameraChanged", ({ view: next }) => {
      view = next.state === "off" ? null : next;
      notify();
    }),
    api.on("serverSynced", ({ snapshot }) => {
      requested.add(snapshot.serverId);
      setUnavailable(snapshot.serverId, undefined);
      read(api, snapshot.serverId);
    }),
    api.on("connectionStateChanged", ({ serverId, state }) => {
      if (state.state === "connected") return;
      requested.delete(serverId);
      if (unavailable.has(serverId)) setUnavailable(serverId, undefined);
    }),
  ];
  api.cameraView().then(
    (current) => {
      if (boundApi === api && view === null && current?.state !== "off") {
        view = current;
        notify();
      }
    },
    (error: unknown) => {
      console.error("Reading the camera state failed", error);
    },
  );
}

function ensureBound(): void {
  const api = getApi();
  if (api !== boundApi) bind(api);
}

function subscribe(listener: () => void): () => void {
  ensureBound();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getView(): CameraView | null {
  ensureBound();
  return view;
}

function reasonOf(serverId: number | null): CameraUnavailable | null {
  ensureBound();
  return (serverId === null ? undefined : unavailable.get(serverId)) ?? null;
}

// Connected before this store was first used: read the server once, as the event only reports
// changes.
function requestOnce(serverId: number | null): void {
  ensureBound();
  if (serverId === null || boundApi === undefined || requested.has(serverId)) return;
  requested.add(serverId);
  read(boundApi, serverId);
}

/** The own camera while it is not off (preview, requesting or on air), else `null`. */
export function useOwnCamera(): CameraView | null {
  return useSyncExternalStore(subscribe, getView);
}

/** Why camera sharing is unavailable on `serverId` (FR-8.4), or `null`. */
export function useCameraUnavailable(serverId: number | null): CameraUnavailable | null {
  useEffect(() => {
    requestOnce(serverId);
  }, [serverId]);
  return useSyncExternalStore(subscribe, () => reasonOf(serverId));
}

/**
 * What stops turning the camera on for `serverId`, first match wins: not in a voice channel there
 * (FR-8.16), then the server's FR-8.4 reason. `null` when it can be turned on.
 */
export function cameraBlock(
  serverId: number | null,
  voiceServerId: number | null,
  reason: CameraUnavailable | null,
): CameraBlock | null {
  if (serverId === null || serverId !== voiceServerId) return "notInChannel";
  return reason;
}

/** The text for a block, as a key of the `camera` namespace. */
export function cameraBlockKey(block: CameraBlock): string {
  return block === "notInChannel" ? "notInChannel" : `unavailable.${block}`;
}

/** Test helper: forgets the binding, the availability and the own camera. */
export function resetCameraAvailability(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  unavailable = new Map();
  requested.clear();
  view = null;
}
