/**
 * Files dragged from the file manager over the channel chat (mockup 1a). In the desktop app the
 * native drag-and-drop event reports the position and the paths; the whole chat column counts as
 * the drop zone. Without native drops (browser dev) there are no paths, so nothing happens.
 */
import { useEffect, useRef, useState, type RefObject } from "react";
import { hasNativeFileDrop, isOverRect, onNativeFileDrop } from "../../chat/fileDrop";

export interface FileDropOptions {
  /** The chat column that receives drops. */
  host: HTMLElement | null;
  /** Drops become chips (a channel where sharing works). */
  accept: boolean;
  /** Drops land on a chat where files cannot go (private chat); the handler tells the user. */
  rejectOutside: boolean;
  onPaths: (paths: readonly string[]) => void;
  onRejected: () => void;
  /** Region inside the host that keeps its own drop behaviour (the image input of a private chat). */
  exclude: RefObject<HTMLElement | null> | null;
}

/** Whether the drag is currently over the zone, to show the cover. */
export function useFileDrop(options: FileDropOptions): boolean {
  const [over, setOver] = useState(false);
  const latest = useRef(options);
  useEffect(() => {
    latest.current = options;
  });

  useEffect(() => {
    if (!hasNativeFileDrop()) return;
    return onNativeFileDrop((event) => {
      if (event.type === "leave") {
        setOver(false);
        return;
      }
      const { host, accept, rejectOutside, exclude, onPaths, onRejected } = latest.current;
      const inside =
        host !== null &&
        isOverRect(event.position, host.getBoundingClientRect(), window.devicePixelRatio);
      const excluded = exclude?.current ?? null;
      const inExcluded =
        excluded !== null &&
        isOverRect(event.position, excluded.getBoundingClientRect(), window.devicePixelRatio);
      const here = inside && !inExcluded;
      if (event.type === "drop") {
        setOver(false);
        if (!here) return;
        if (accept) onPaths(event.paths);
        else if (rejectOutside) onRejected();
        return;
      }
      setOver(here && accept);
    });
  }, []);

  return over;
}
