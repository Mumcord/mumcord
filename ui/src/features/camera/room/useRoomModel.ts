/**
 * Everything the camera room shows, in one place (spec 08 §4.8): the tiles in their stable order
 * with name, talking and microphone state, the counts of the header and the people without a
 * camera. The views below only render this model.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import type {
  CameraTile,
  CameraTileQuality,
  CameraTileState,
  CameraView,
  User,
} from "../../../api";
import { useServerStateOf } from "../../channels";
import { voiceState } from "../../channels/statusText";
import { SPEAKER_HOLD_MS, speakerTile, stableOrder, withPinnedFirst, type TileKey } from "./layout";
import { useRoomChoices, type RoomLayout } from "./roomChoices";
import { useCameraRoom, useOwnCamera } from "./roomStore";

export interface TileModel {
  key: TileKey;
  /** The camera owner's session; the own session for the self-view. */
  session: number;
  name: string;
  /** `self` is the own camera, drawn from local frames. */
  state: CameraTileState | "self";
  speaking: boolean;
  /** Muted or deafened, by themselves or the server (FR-8.65). */
  micOff: boolean;
  /** The "{n} fps" badge of a weak connection (FR-8.58). */
  fpsBadge: number | null;
  quality: CameraTileQuality;
  pinned: boolean;
  user: User | undefined;
}

export interface RoomModel {
  serverId: number;
  /** Cameras on in the channel, the own one included (FR-8.62). */
  cameraCount: number;
  peopleCount: number;
  channelName: string;
  /** The tiles shown, in their order; the self-view is left out while hidden. */
  tiles: TileModel[];
  /** The Speaker layout's large tile (FR-8.64). */
  speaker: TileKey | null;
  /** The tile shown large by the tile menu ("Show large"), while it is shown. */
  large: TileKey | null;
  layout: RoomLayout;
  selfHidden: boolean;
  own: CameraView | null;
  /** People in the channel without a camera, plain Mumble users included (FR-8.67). */
  withoutCamera: User[];
}

const NO_TILES: readonly CameraTile[] = [];

/** Who spoke last for at least 1.5 s among `keys` (FR-8.64); a short "yes" changes nothing. */
function useLastSpeaker(talking: ReadonlySet<TileKey>): TileKey | null {
  const [last, setLast] = useState<TileKey | null>(null);
  const timers = useRef(new Map<TileKey, ReturnType<typeof setTimeout>>());
  useEffect(() => {
    const running = timers.current;
    for (const [key, timer] of running) {
      if (talking.has(key)) continue;
      clearTimeout(timer);
      running.delete(key);
    }
    for (const key of talking) {
      if (running.has(key)) continue;
      running.set(
        key,
        setTimeout(() => {
          setLast(key);
        }, SPEAKER_HOLD_MS),
      );
    }
  }, [talking]);
  useEffect(() => {
    const running = timers.current;
    return () => {
      for (const timer of running.values()) clearTimeout(timer);
      running.clear();
    };
  }, []);
  return last;
}

export function useRoomModel(serverId: number): RoomModel {
  const room = useCameraRoom(serverId);
  const own = useOwnCamera(serverId);
  const { users, channels, selfSession, talking } = useServerStateOf(serverId);
  const choices = useRoomChoices();
  // What the last render showed, kept the way React allows: set during render when it changed.
  const [shownOrder, setShownOrder] = useState<readonly TileKey[]>([]);
  const [shownSpeaker, setShownSpeaker] = useState<TileKey | null>(null);

  const self = selfSession === null ? undefined : users.get(selfSession);
  const onAir = own?.state === "onAir";
  const tiles = room?.tiles ?? NO_TILES;

  const present: TileKey[] = tiles.map((tile) => tile.sharer);
  if (onAir) present.push("self");
  // A stable order across renders: the order in which cameras turned on (FR-8.63).
  const order = stableOrder(shownOrder, present);
  if (order !== shownOrder) setShownOrder(order);

  const talkingKeys = useMemo(() => {
    const keys = new Set<TileKey>();
    for (const tile of tiles) if (talking.has(tile.sharer)) keys.add(tile.sharer);
    if (onAir && selfSession !== null && talking.has(selfSession)) keys.add("self");
    return keys;
  }, [tiles, talking, onAir, selfSession]);
  const lastSpeaker = useLastSpeaker(talkingKeys);

  const byKey = new Map(tiles.map((tile) => [tile.sharer, tile]));
  const models: TileModel[] = [];
  for (const key of withPinnedFirst(order, choices.pinned)) {
    if (key === "self") {
      if (choices.selfHidden || selfSession === null) continue;
      models.push({
        key,
        session: selfSession,
        name: self?.name ?? "",
        state: "self",
        speaking: talking.has(selfSession),
        micOff: self !== undefined && voiceState(self) !== null,
        fpsBadge: null,
        quality: "auto",
        pinned: choices.pinned.includes(key),
        user: self,
      });
      continue;
    }
    const tile = byKey.get(key);
    if (tile === undefined) continue;
    const user = users.get(tile.sharer);
    models.push({
      key,
      session: tile.sharer,
      name: user?.name ?? "",
      state: tile.hidden ? "hidden" : tile.state,
      speaking: talking.has(tile.sharer),
      micOff: user !== undefined && voiceState(user) !== null,
      fpsBadge: tile.fpsBadge,
      quality: tile.quality,
      pinned: choices.pinned.includes(key),
      user,
    });
  }

  const shownKeys = models.map((model) => model.key);
  const speaker = speakerTile(shownKeys, choices.pinned, lastSpeaker, shownSpeaker);
  if (speaker !== shownSpeaker) setShownSpeaker(speaker);

  const channelId = self?.channelId;
  const inChannel =
    channelId === undefined ? [] : [...users.values()].filter((u) => u.channelId === channelId);
  const withCamera = new Set(tiles.map((tile) => tile.sharer));
  const withoutCamera = inChannel.filter(
    (u) => u.session !== selfSession && !withCamera.has(u.session),
  );

  return {
    serverId,
    cameraCount: tiles.length + (onAir ? 1 : 0),
    peopleCount: inChannel.length,
    channelName: channelId === undefined ? "" : (channels.get(channelId)?.name ?? ""),
    tiles: models,
    speaker,
    large: choices.large !== null && shownKeys.includes(choices.large) ? choices.large : null,
    layout: choices.layout,
    selfHidden: choices.selfHidden,
    own,
    withoutCamera,
  };
}
