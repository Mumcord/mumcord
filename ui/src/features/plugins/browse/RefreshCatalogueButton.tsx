import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../../api";
import { refreshPlugins } from "../pluginStore";
import { pluginErrorText, pluginMessageText } from "../prompt/errorText";
import { currentPluginIndex, loadPluginIndex } from "./indexStore";

type Outcome = { kind: "updated" | "offline" } | { kind: "error"; text: string };

/**
 * "Refresh catalogue" (Browse and Repositories tabs): loads the official list (mirror setting
 * applies) and every switched-on repository again, then checks for plugin updates so the
 * Updates tab is current too. The engine always asks the server whether its copy is still
 * current, so one load is enough. Shows progress and the result next to the button.
 */
export function RefreshCatalogueButton() {
  const { t, i18n } = useTranslation("pluginBrowse");
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<Outcome | null>(null);

  const refresh = async () => {
    setBusy(true);
    setOutcome(null);
    try {
      await loadPluginIndex(i18n.language);
      const state = currentPluginIndex();
      if (state.status !== "loaded") {
        setOutcome({
          kind: "error",
          text:
            state.status === "failed" && state.message !== ""
              ? pluginMessageText(t, state.message)
              : t("errors.unreachable"),
        });
        return;
      }
      if (state.index.freshness === "offline") {
        setOutcome({ kind: "offline" });
        return;
      }
      if (state.index.notice !== null) {
        setOutcome({ kind: "error", text: pluginMessageText(t, state.index.notice) });
        return;
      }
      try {
        await getApi().pluginCheckNow();
        setOutcome({ kind: "updated" });
      } catch (error: unknown) {
        console.error("Checking for plugin updates failed", error);
        setOutcome({
          kind: "error",
          text: pluginErrorText(t, error, t("refresh.checkFailed")),
        });
      }
      await refreshPlugins();
    } finally {
      setBusy(false);
    }
  };

  const text =
    outcome === null ? "" : outcome.kind === "error" ? outcome.text : t(`refresh.${outcome.kind}`);

  return (
    <div className="plugin-refresh">
      <button
        type="button"
        className="settings-button"
        aria-busy={busy}
        disabled={busy}
        onClick={() => void refresh()}
      >
        {busy && <span className="plugin-refresh__spinner" aria-hidden="true" />}
        {t("refresh.button")}
      </button>
      <span
        role={outcome?.kind === "error" ? "alert" : "status"}
        className="plugin-refresh__result"
      >
        {text}
      </span>
    </div>
  );
}
