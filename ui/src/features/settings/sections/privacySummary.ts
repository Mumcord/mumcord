import type { TFunction } from "i18next";
import type { Settings } from "../../../api";
import type { SettingEntry } from "../settingsIndex";

export interface SummaryLine {
  /** The card the line leads to; its title also names the line's link. */
  target: SettingEntry;
  text: string;
}

/** Plain-language lines of the summary, in the order of the mockup (#161). */
export function summaryLines(settings: Settings, t: TFunction<"settings">): SummaryLine[] {
  const p2p =
    settings.p2pMode === "relayOnly" || settings.p2pMode === "relayFirst"
      ? "privacySummary.p2pHidden"
      : settings.p2pMode === "directOnly"
        ? "privacySummary.p2pDirectOnly"
        : "privacySummary.p2pDirect";
  const sharing =
    settings.nowPlaying && settings.nowGaming
      ? "privacySummary.sharingBoth"
      : settings.nowPlaying
        ? "privacySummary.sharingPlaying"
        : settings.nowGaming
          ? "privacySummary.sharingGaming"
          : "privacySummary.sharingNone";
  return [
    {
      target: { section: "chatNotifications", group: "privateMessages.title" },
      text: t("privacySummary.messages", {
        rule: t(`privateMessages.${settings.privateMessages.rule}`),
      }),
    },
    {
      target: { section: "privacy", group: "privacyConnections.title" },
      text: t(p2p),
    },
    {
      target: { section: "privacy", group: "externalContent.title" },
      text: `${t(settings.linkPreviews ? "privacySummary.externalOn" : "privacySummary.externalOff")} ${t(
        settings.offerFullImages || settings.loadFullImages
          ? "privacySummary.imagesOn"
          : "privacySummary.imagesOff",
      )}`,
    },
    {
      target: { section: "privacy", group: "historySharing.title" },
      text: t(settings.historySharing ? "privacySummary.historyOn" : "privacySummary.historyOff"),
    },
    {
      target: { section: "account", group: "nowPlaying.title" },
      text: t(sharing),
    },
  ];
}
