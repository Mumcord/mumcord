/**
 * The party's YouTube player for the cinema view (issue #163, FR-2.41a). It runs in the app's
 * loopback player page because YouTube refuses to play inside the app's own origin; the hook drives
 * the bridge, keeps the local player in step with the party (`PartySync`), and knows what the seek
 * bar needs: the position, the length, and how much the player has.
 *
 * The player also says where it is, so the watching list can show each person (issue #163): at once
 * when it starts buffering, otherwise at most every {@link REPORT_MS} while it plays.
 */
import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { getApi, type WatchCommand } from "../../api";
import { PartySync } from "./partySync";
import { ducker } from "./ducking";
import {
  loadCommand,
  parseBridgeReport,
  playerOrigin,
  userVolumeCommand,
  volumeCommand,
  type BridgeCommand,
} from "./playerBridge";
import type { LiveParty } from "./watchPartyStore";
import { BUFFERING, ENDED, PLAYING } from "./youtube";

/** How long the page may stay silent before the view says it cannot play (FR-2.41a). */
export const READY_TIMEOUT_MS = 8000;
/** How often a playing player tells the party where it is (issue #163). */
export const REPORT_MS = 10000;

export interface WatchPlayerOptions {
  /** A playback change the user made, for everyone. */
  onCommand: (command: WatchCommand) => void;
  /** The video of playlist entry `entry` ended here. */
  onEnded: (entry: number) => void;
  /** Where this player is (issue #163). */
  onReport: (positionMs: number, playing: boolean, buffering: boolean) => void;
}

export interface WatchPlayer {
  pageUrl: string | null;
  /** The load command for the current video, sent when the page answers. */
  load: BridgeCommand | null;
  /** Whether the player answered. */
  ready: boolean;
  failed: boolean;
  youTubeError: number | null;
  /** The player's own state code. */
  state: number | null;
  positionMs: number;
  durationMs: number;
  /** How much the player has, or `null` where it does not say. */
  bufferedMs: number | null;
  /** The viewer's own volume as a fraction (issue #163). */
  volume: number;
  play: () => void;
  pause: () => void;
  seekTo: (positionMs: number) => void;
  setVolume: (level: number) => void;
}

export function useWatchPlayer(
  live: LiveParty,
  options: WatchPlayerOptions,
  frameRef: RefObject<HTMLIFrameElement | null>,
): WatchPlayer {
  const [pageUrl, setPageUrl] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [failed, setFailed] = useState(false);
  const [youTubeError, setYouTubeError] = useState<number | null>(null);
  const [state, setState] = useState<number | null>(null);
  const [positionMs, setPositionMs] = useState(live.party.positionMs);
  const [durationMs, setDurationMs] = useState(0);
  const [bufferedMs, setBufferedMs] = useState<number | null>(null);
  const [volume, setLevel] = useState(1);
  const videoId = live.party.videoId;
  const origin = pageUrl === null ? null : playerOrigin(pageUrl);
  const postRef = useRef<(command: BridgeCommand) => void>(() => {});
  const syncRef = useRef<PartySync | null>(null);
  const volumeRef = useRef(volume);
  const optionsRef = useRef(options);
  const liveRef = useRef(live);
  const localRef = useRef({ ms: live.party.positionMs, state: null as number | null });
  useEffect(() => {
    volumeRef.current = volume;
  }, [volume]);
  useEffect(() => {
    optionsRef.current = options;
  }, [options]);
  useEffect(() => {
    liveRef.current = live;
  }, [live]);

  useEffect(() => {
    let current = true;
    getApi()
      .watchPartyPlayerUrl()
      .then((url) => {
        if (!current) return;
        if (playerOrigin(url) === null) {
          console.warn("Unexpected watch party player page");
          setFailed(true);
        } else {
          setPageUrl(url);
        }
      })
      .catch((error: unknown) => {
        console.warn("Watch party player page unavailable", error);
        if (current) setFailed(true);
      });
    return () => {
      current = false;
    };
  }, []);

  useEffect(() => {
    if (origin === null) return;
    const post = (command: BridgeCommand | null) => {
      if (command === null) return;
      frameRef.current?.contentWindow?.postMessage(command, origin);
    };
    postRef.current = post;
    const control = new PartySync({
      now: () => Date.now(),
      player: {
        play: () => { post({ type: "play" }); },
        pause: () => { post({ type: "pause" }); },
        seekTo: (ms) => { post({ type: "seek", seconds: ms / 1000 }); },
      },
      send: (command) => { optionsRef.current.onCommand(command); },
      ended: (entry) => { optionsRef.current.onEnded(entry); },
    });
    syncRef.current = control;
    control.update(liveRef.current);
    let answered = false;
    let lastReport = 0;
    const report = (buffering: boolean) => {
      if (!buffering && lastReport !== 0 && Date.now() - lastReport < REPORT_MS) return;
      lastReport = Date.now();
      optionsRef.current.onReport(
        localRef.current.ms,
        localRef.current.state === PLAYING,
        buffering,
      );
    };
    const onMessage = (event: MessageEvent) => {
      if (event.origin !== origin || event.source !== frameRef.current?.contentWindow) return;
      const message = parseBridgeReport(event.data);
      if (message === null) return;
      if (message.type === "error") {
        console.warn(`YouTube player error ${String(message.code)}`);
        answered = true;
        setYouTubeError(message.code);
        setFailed(true);
        return;
      }
      if (!answered) {
        answered = true;
        setReady(true);
        control.onReady();
        post(userVolumeCommand(volumeRef.current));
        if (ducker.level !== 1) post(volumeCommand(ducker.level));
      }
      switch (message.type) {
        case "state":
        case "ended": {
          const code = message.type === "state" ? message.state : ENDED;
          localRef.current.state = code;
          setState(code);
          control.onState(code);
          report(code === BUFFERING);
          break;
        }
        case "time": {
          localRef.current.ms = Math.round(message.seconds * 1000);
          setPositionMs(localRef.current.ms);
          control.onTime(message.seconds);
          report(false);
          break;
        }
        case "meta":
          setDurationMs(Math.round(message.seconds * 1000));
          if (message.bufferedSeconds !== null) {
            setBufferedMs(Math.round(message.bufferedSeconds * 1000));
          }
          break;
        default:
      }
    };
    window.addEventListener("message", onMessage);
    // Ducking while people talk (FR-2.76); the page fades and keeps the viewer's own volume.
    const unsubscribeDucking = ducker.subscribe((level) => {
      if (answered) post(volumeCommand(level));
    });
    const timeout = window.setTimeout(() => {
      if (!answered) setFailed(true);
    }, READY_TIMEOUT_MS);
    return () => {
      window.removeEventListener("message", onMessage);
      unsubscribeDucking();
      postRef.current = () => {};
      syncRef.current = null;
      window.clearTimeout(timeout);
    };
  }, [videoId, origin]);

  /** The user pressed a control: it goes to the player and to everyone (issue #163). */
  const act = useCallback((command: WatchCommand, toPlayer: BridgeCommand) => {
    postRef.current(toPlayer);
    syncRef.current?.act(command);
  }, []);
  const play = useCallback(
    () => { act({ kind: "play", positionMs: localRef.current.ms }, { type: "play" }); },
    [act],
  );
  const pause = useCallback(
    () => { act({ kind: "pause", positionMs: localRef.current.ms }, { type: "pause" }); },
    [act],
  );
  const seekTo = useCallback(
    (ms: number) => { act({ kind: "seek", positionMs: ms }, { type: "seek", seconds: ms / 1000 }); },
    [act],
  );
  const setVolume = useCallback(
    (level: number) => {
      setLevel(level);
      const command = userVolumeCommand(level);
      if (command !== null) postRef.current(command);
    },
    [],
  );

  return {
    pageUrl,
    load: loadCommand(videoId),
    ready,
    failed,
    youTubeError,
    state,
    positionMs,
    durationMs,
    bufferedMs,
    volume,
    play,
    pause,
    seekTo,
    setVolume,
  };
}
