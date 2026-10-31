/**
 * The preview of a party video (issue #163): title and thumbnail come from the engine's link
 * preview, the same one a chat card shows, so nothing else is asked. Only joined participants get
 * this view, and with embeds off the view is not mounted at all (FR-3.123), so nothing is fetched
 * before the user joins.
 */
import { useEffect, useState } from "react";
import type { LinkPreview } from "../../api";
import { loadPreview } from "../chat/links";
import { watchUrl } from "./youtube";

export function useVideoPreview(videoId: string): LinkPreview | null {
  const [preview, setPreview] = useState<{ videoId: string; preview: LinkPreview | null }>();
  useEffect(() => {
    let current = true;
    void loadPreview(watchUrl(videoId)).then((loaded) => {
      if (current) setPreview({ videoId, preview: loaded });
    });
    return () => {
      current = false;
    };
  }, [videoId]);
  return preview?.videoId === videoId ? preview.preview : null;
}
