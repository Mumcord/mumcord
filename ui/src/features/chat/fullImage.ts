/**
 * Full-size images between Mumcord users (FR-1.60c). The chat message carries a thumbnail that
 * fits the server limit; when it had to be shrunk, the engine keeps the original and the
 * thumbnail's `<img>` names it with a reference (content hash and size). Official clients ignore
 * the attribute. The image viewer asks the engine for the full image, which fetches it from the
 * original sender on demand; until it arrives (or if it never does) the thumbnail stays.
 */
import { useEffect, useState, useSyncExternalStore } from "react";
import { getApi } from "../../api";
import { shownServerId } from "../channels/serverStore";

/** Attribute on the message's `<img>` that carries the reference. */
export const FULL_ATTRIBUTE = "data-mumcord-full";

/** `<sha256 hex>:<bytes>`, exactly as the engine writes it. */
export const FULL_REFERENCE = /^[0-9a-f]{64}:[1-9][0-9]{0,9}$/;

/** Characters the attribute may add to the message; reserved in the image budget. */
export const FULL_ATTRIBUTE_RESERVE = ` ${FULL_ATTRIBUTE}=""`.length + 64 + 1 + 10;

/** Where a full image can be fetched from: its reference and the message's sender. */
export interface FullImageSource {
  reference: string;
  senderSession: number | null;
  senderCertHash: string | null;
}

function readAsDataUrl(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      resolve(typeof reader.result === "string" ? reader.result : "");
    };
    reader.onerror = () => {
      reject(reader.error ?? new Error("reading the image failed"));
    };
    reader.readAsDataURL(file);
  });
}

/**
 * Lets the engine keep the original of a picture whose thumbnail had to be shrunk. Returns the
 * reference for the thumbnail, or `null` when the thumbnail already is the original or the
 * original cannot be kept (the message then simply carries the thumbnail), also while
 * "Offer my full-size images" is off (FR-1.74).
 */
export async function keepFullImage(file: Blob, thumbnail: string): Promise<string | null> {
  try {
    // Read at send time so a change in Settings applies to the very next picture.
    if (!(await getApi().settingsGet()).offerFullImages) return null;
    const original = await readAsDataUrl(file);
    if (original === thumbnail) return null;
    const reference = await getApi().fullImageKeep(original);
    return FULL_REFERENCE.test(reference) ? reference : null;
  } catch (error) {
    console.error("Keeping the full image failed; sending the thumbnail only", error);
    return null;
  }
}

/** The `<img>` tag of a sent image, with the full-image reference when there is one. */
export function fullImageTag(dataUrl: string, reference: string | null): string {
  return reference === null
    ? `<img src="${dataUrl}" />`
    : `<img src="${dataUrl}" ${FULL_ATTRIBUTE}="${reference}" />`;
}

/** Where a rendered message image's full version comes from, if it names one. */
export function fullImageSource(img: HTMLImageElement): FullImageSource | undefined {
  const reference = img.getAttribute(FULL_ATTRIBUTE);
  if (reference === null || !FULL_REFERENCE.test(reference)) return undefined;
  const session = img.dataset["senderSession"];
  return {
    reference,
    senderSession: session !== undefined && /^\d+$/.test(session) ? Number(session) : null,
    senderCertHash: img.dataset["senderCert"] ?? null,
  };
}

/** Fetches in flight or done, by reference: opening the viewer again reuses them. */
const fetches = new Map<string, Promise<string | null>>();

/** References whose fetch has not finished yet, with the last known progress (0–1, or null). */
const running = new Map<string, number | null>();
const runningListeners = new Set<() => void>();
let runningSnapshot: ReadonlyMap<string, number | null> = new Map();

function setRunning(change: (map: Map<string, number | null>) => void): void {
  change(running);
  runningSnapshot = new Map(running);
  for (const listener of runningListeners) listener();
}

function fetchFullImage(source: FullImageSource): Promise<string | null> {
  const known = fetches.get(source.reference);
  if (known) return known;
  setRunning((map) => {
    map.set(source.reference, null);
  });
  const started = getApi()
    .fullImageFetch(shownServerId(), source.reference, source.senderSession, source.senderCertHash)
    .catch((error: unknown) => {
      console.error("Fetching the full image failed", error);
      return null;
    })
    .then((src) => {
      setRunning((map) => {
        map.delete(source.reference);
      });
      // A failure may be temporary (sender offline): allow a new try on the next open.
      if (src === null) fetches.delete(source.reference);
      return src;
    });
  fetches.set(source.reference, started);
  return started;
}

/** "Load full-size images from others" (FR-1.74); on when the setting cannot be read. */
async function loadAllowed(): Promise<boolean> {
  try {
    return (await getApi().settingsGet()).loadFullImages;
  } catch (error) {
    console.error("Reading the settings failed; loading full images", error);
    return true;
  }
}

/** Forgets finished and running fetches (tests). */
export function resetFullImages(): void {
  fetches.clear();
  setRunning((map) => {
    map.clear();
  });
}

/**
 * Stops a running transfer on purpose (the × on the progress bar). Closing the viewer never
 * does: the transfer goes on in the background (FR-2.38).
 */
export function cancelFullImage(reference: string): void {
  getApi()
    .fullImageCancel(shownServerId(), reference)
    .catch((error: unknown) => {
      console.error("Cancelling the full image failed", error);
    });
}

/**
 * Progress (0–1) of the full images that are loading right now, by reference; `null` while it
 * started but nothing arrived yet. Shown as a slim bar under the image in chat (FR-2.22).
 */
export function useLoadingFullImages(): ReadonlyMap<string, number | null> {
  return useSyncExternalStore(subscribeRunning, () => runningSnapshot);
}

let stopEvents: (() => void) | null = null;

/** One engine listener serves all bars, attached while any bar exists. */
function subscribeRunning(listener: () => void): () => void {
  runningListeners.add(listener);
  stopEvents ??= getApi().on("fullImageProgress", (event) => {
    if (!running.has(event.reference) || event.total === 0) return;
    const progress = Math.min(1, event.received / event.total);
    setRunning((map) => {
      map.set(event.reference, progress);
    });
  });
  return () => {
    runningListeners.delete(listener);
    if (runningListeners.size === 0) {
      stopEvents?.();
      stopEvents = null;
    }
  };
}

export type FullImageState =
  | { status: "none" }
  | { status: "loading"; progress: number | null }
  | { status: "done"; src: string }
  | { status: "failed" };

/** The full image for `source` (if any): loading with progress (0–1), done or failed. */
export function useFullImage(source: FullImageSource | undefined): FullImageState {
  const reference = source?.reference;
  const senderSession = source?.senderSession ?? null;
  const senderCertHash = source?.senderCertHash ?? null;
  const [state, setState] = useState<{ reference: string; value: FullImageState } | null>(null);

  useEffect(() => {
    if (reference === undefined) return;
    // Until the first event the result below reads "loading" (state of another reference).
    let active = true;
    const stop = getApi().on("fullImageProgress", (event) => {
      if (!active || event.reference !== reference || event.total === 0) return;
      const progress = Math.min(1, event.received / event.total);
      // Kept for a later reopen, also when no chat bar listens.
      if (running.has(reference)) {
        setRunning((map) => {
          map.set(reference, progress);
        });
      }
      setState({ reference, value: { status: "loading", progress } });
    });
    void loadAllowed()
      .then((allowed) =>
        allowed ? fetchFullImage({ reference, senderSession, senderCertHash }) : undefined,
      )
      .then((src) => {
        if (!active) return;
        setState({
          reference,
          value:
            src === undefined
              ? { status: "none" }
              : src === null
                ? { status: "failed" }
                : { status: "done", src },
        });
      });
    return () => {
      active = false;
      stop();
    };
  }, [reference, senderSession, senderCertHash]);

  if (reference === undefined) return { status: "none" };
  // A transfer that is already running (viewer reopened) shows the progress it has made.
  if (state?.reference !== reference)
    return { status: "loading", progress: runningSnapshot.get(reference) ?? null };
  return state.value;
}
