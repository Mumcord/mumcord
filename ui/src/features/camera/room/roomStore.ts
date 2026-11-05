/**
 * The camera room as the engine sees it (spec 08 §4.7–4.8): the other people's camera tiles of
 * each server (`camera_room` + `cameraRoomChanged`) and the own camera (`camera_view` +
 * `cameraChanged`). Main window and camera pop-out both read it; an event that arrives before the
 * first read wins over that read.
 */
import { useCallback, useEffect, useSyncExternalStore } from "react";
import {
  getApi,
  type CameraRoomView,
  type CameraView,
  type MumcordApi,
  type Unsubscribe,
} from "../../../api";

let rooms: ReadonlyMap<number, CameraRoomView> = new Map();
let own: CameraView | null = null;
/** Servers whose room was read or changed since binding, so the read is not repeated. */
const known = new Set<number>();
let ownChanged = false;
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

function setRoom(view: CameraRoomView): void {
  known.add(view.serverId);
  const next = new Map(rooms);
  next.set(view.serverId, view);
  rooms = next;
  notify();
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  rooms = new Map();
  own = null;
  known.clear();
  ownChanged = false;
  unsubscribers = [
    api.on("cameraRoomChanged", ({ view }) => {
      setRoom(view);
    }),
    api.on("cameraChanged", ({ view }) => {
      ownChanged = true;
      own = view;
      notify();
    }),
  ];
  api.cameraView().then(
    (view) => {
      if (boundApi !== api || ownChanged) return;
      own = view;
      notify();
    },
    (error: unknown) => {
      console.error("Reading the own camera failed", error);
    },
  );
}

function ensureBound(): MumcordApi {
  const api = getApi();
  if (api !== boundApi) bind(api);
  return api;
}

function load(serverId: number): void {
  const api = ensureBound();
  if (known.has(serverId)) return;
  known.add(serverId);
  api.cameraRoom(serverId).then(
    (view) => {
      if (boundApi === api && !rooms.has(serverId)) setRoom(view);
    },
    (error: unknown) => {
      known.delete(serverId);
      console.error("Reading the camera room failed", error);
    },
  );
}

function subscribe(listener: () => void): () => void {
  ensureBound();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The camera room of `serverId`; `null` until the engine answered. */
export function useCameraRoom(serverId: number | null): CameraRoomView | null {
  useEffect(() => {
    if (serverId !== null) load(serverId);
  }, [serverId]);
  const get = useCallback(
    () => (serverId === null ? null : (rooms.get(serverId) ?? null)),
    [serverId],
  );
  return useSyncExternalStore(subscribe, get);
}

/** The own camera when it belongs to `serverId`; `null` while it is off or on another server. */
export function useOwnCamera(serverId: number | null): CameraView | null {
  const get = useCallback(() => {
    ensureBound();
    return own !== null && own.serverId === serverId && own.state !== "off" ? own : null;
  }, [serverId]);
  return useSyncExternalStore(subscribe, get);
}

/** Test helper: forgets the binding and every room. */
export function resetCameraRoomStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  rooms = new Map();
  own = null;
  known.clear();
  ownChanged = false;
}
