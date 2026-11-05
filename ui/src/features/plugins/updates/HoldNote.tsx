import { useTranslation } from "react-i18next";
import type { PluginUpdate, PluginUpdateHold } from "../../../api";

/** Why an update waits instead of installing by itself (FR-9.82), in the words of score mockup 04. */
export function HoldNote({ update, hold }: { update: PluginUpdate; hold: PluginUpdateHold }) {
  const { t } = useTranslation("pluginUpdates");
  const text = (() => {
    switch (hold.kind) {
      case "newWebsites":
        return t("hold.newWebsites", {
          count: hold.count,
          name: update.name,
          version: update.from,
        });
      case "riskUp":
        return t("hold.riskUp", { from: t(`band.${hold.from}`), to: t(`band.${hold.to}`) });
      case "scoreUp":
        return t("hold.scoreUp", { from: hold.from, to: hold.to });
      case "blockedBySafety":
        return t("hold.blockedBySafety");
      case "scoreUnknown":
        return t("hold.scoreUnknown");
    }
  })();
  return <p className="plugin-update__hold">{text}</p>;
}
