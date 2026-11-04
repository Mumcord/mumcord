/** What a share with the dialog's picks would get, for every codec choice (FR-7.36). */
import { useEffect, useState } from "react";
import { getApi, type QualityChoice, type ScreenCodec, type SharePreview } from "../../../api";
import { CODECS } from "./startPicks";

export type SharePreviews = Partial<Record<ScreenCodec, SharePreview>>;

/**
 * Asks the engine for the preview of each codec option, so the codec list can name the encoder
 * every option would use and the "Viewers get" line follows the pick at once. Answers to older
 * picks are dropped, so a slow answer never overwrites a newer one.
 */
export function useSharePreviews(serverId: number, choice: QualityChoice): SharePreviews {
  const [previews, setPreviews] = useState<SharePreviews>({});
  const { resolution, fps, optimize, uploadLimit } = choice;

  useEffect(() => {
    let current = true;
    const picks = { resolution, fps, optimize, uploadLimit };
    Promise.all(CODECS.map((codec) => getApi().screenSharePreview(serverId, picks, codec))).then(
      (answers) => {
        if (!current) return;
        setPreviews(Object.fromEntries(CODECS.map((codec, i) => [codec, answers[i]])));
      },
      (error: unknown) => {
        console.error("The screen share preview failed", error);
        if (current) setPreviews({});
      },
    );
    return () => {
      current = false;
    };
  }, [serverId, resolution, fps, optimize, uploadLimit]);

  return previews;
}
