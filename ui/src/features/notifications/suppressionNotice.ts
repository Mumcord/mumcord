/**
 * Says so when the selected noise suppression is too slow for this computer and the standard one
 * stands in (#753), bottom right like a message. Without it the user would hear no difference and
 * wonder why the level they chose is not at work.
 */
import type { TFunction } from "i18next";
import type { MumcordApi, Unsubscribe } from "../../api";
import { showToast } from "./notificationStore";

/** The notice belongs to the microphone, not to a server or a chat. */
const KEY = "noise-suppression-fallback";

export function bindSuppressionNotices(
  api: MumcordApi,
  context: () => { t: TFunction<"notifications">; tSettings: TFunction<"settings"> },
): Unsubscribe {
  return api.on("noiseSuppressionFallback", () => {
    const { t, tSettings } = context();
    showToast({
      key: KEY,
      serverId: 0,
      conversation: { kind: "channel", channelId: 0 },
      title: t("suppressionFallback.title"),
      text: t("suppressionFallback.text", { standard: tSettings("voice.noiseChoice.standard") }),
      open: () => undefined,
    });
  });
}
