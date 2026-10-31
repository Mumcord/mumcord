/**
 * Channel audio profiles of the connected servers and the profile in effect (issue #171). One
 * store for the app; it binds to the active engine client and re-binds when tests swap it.
 */
import { useEffect, useSyncExternalStore } from "react";
import {
  getApi,
  type ActiveAudioProfile,
  type ChannelAudioEntry,
  type ChannelAudioProfile,
  type MumcordApi,
  type Unsubscribe,
} from "../../api";

type Profiles = ReadonlyMap<number, ChannelAudioProfile>;

const NONE: Profiles = new Map();

let byServer: ReadonlyMap<number, Profiles> = new Map();
let active: ActiveAudioProfile | null = null;
const requested = new Set<number>();
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

function setProfiles(serverId: number, entries: readonly ChannelAudioEntry[] | null): void {
  const copy = new Map(byServer);
  if (entries === null) copy.delete(serverId);
  else copy.set(serverId, new Map(entries.map((e) => [e.channelId, e.profile])));
  byServer = copy;
  notify();
}

function load(api: MumcordApi, serverId: number): void {
  requested.add(serverId);
  api.channelAudioProfiles(serverId).then(
    (entries) => {
      if (api === boundApi) setProfiles(serverId, entries);
    },
    (error: unknown) => {
      // Not connected (any more): the server's profiles come with its next sync.
      requested.delete(serverId);
      console.warn("Loading the channel audio profiles failed", error);
    },
  );
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  byServer = new Map();
  active = null;
  requested.clear();
  unsubscribers = [
    api.on("channelAudioProfilesChanged", ({ serverId, profiles }) => {
      requested.add(serverId);
      setProfiles(serverId, profiles);
    }),
    api.on("activeAudioProfileChanged", ({ active: next }) => {
      active = next;
      notify();
    }),
    api.on("serverSynced", ({ snapshot }) => {
      load(api, snapshot.serverId);
    }),
    api.on("connectionStateChanged", ({ serverId, state }) => {
      if (state.state !== "disconnected") return;
      requested.delete(serverId);
      setProfiles(serverId, null);
    }),
  ];
  api.activeAudioProfile().then(
    (current) => {
      if (api !== boundApi) return;
      active = current;
      notify();
    },
    (error: unknown) => {
      console.error("Loading the active audio profile failed", error);
    },
  );
}

function ensureBound(): MumcordApi {
  const api = getApi();
  if (api !== boundApi) bind(api);
  return api;
}

function subscribe(listener: () => void): () => void {
  ensureBound();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getProfiles(): ReadonlyMap<number, Profiles> {
  ensureBound();
  return byServer;
}

function getActive(): ActiveAudioProfile | null {
  ensureBound();
  return active;
}

/** The audio profiles of a connected server's channels, by channel id. */
export function useChannelAudioProfiles(serverId: number | null): Profiles {
  const all = useSyncExternalStore(subscribe, getProfiles);
  useEffect(() => {
    if (serverId === null) return;
    const api = ensureBound();
    if (!requested.has(serverId)) load(api, serverId);
  }, [serverId]);
  return serverId === null ? NONE : (all.get(serverId) ?? NONE);
}

/** The profile of one channel; `null` without one. */
export function useChannelAudioProfile(
  serverId: number | null,
  channelId: number,
): ChannelAudioProfile | null {
  return useChannelAudioProfiles(serverId).get(channelId) ?? null;
}

/** The channel profile in effect (own channel on the voice server); `null` runs on the globals. */
export function useActiveAudioProfile(): ActiveAudioProfile | null {
  return useSyncExternalStore(subscribe, getActive);
}

/** Saves (or with `null` removes) a channel's profile; the engine applies it if the user is there. */
export function saveChannelAudioProfile(
  serverId: number,
  channelId: number,
  profile: ChannelAudioProfile | null,
): Promise<null> {
  return getApi().channelAudioProfileSet(serverId, channelId, profile);
}
