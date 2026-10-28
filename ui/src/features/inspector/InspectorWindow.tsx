import { useEffect, useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type InspectorView } from "../../api";
import { applyLanguage, resolveLanguage } from "../../i18n";
import { applyRootLook, isCloseShortcut, namesOf } from "./inspectorView";
import { PacketInspector } from "./PacketInspector";

/**
 * Root of the packet inspector's own window (issue #60). It renders only the inspector and
 * takes theme, language and names from the main window.
 */
export function InspectorWindow() {
  const { t } = useTranslation("settings");
  const titleId = useId();
  const [view, setView] = useState<InspectorView | null>(null);

  useEffect(() => {
    const api = getApi();
    let updated = false;
    const stop = api.on("inspectorViewChanged", (changed) => {
      updated = true;
      setView(changed.view);
    });
    api.packetInspectorView().then(
      (shared) => {
        if (!updated && shared !== null) setView(shared);
      },
      (error: unknown) => {
        console.error("Loading the main window's look failed", error);
      },
    );
    return stop;
  }, []);

  useEffect(() => {
    if (view === null) return;
    applyRootLook(document.documentElement, view);
    applyLanguage(resolveLanguage(null, [view.language])).catch((error: unknown) => {
      console.error("Switching the UI language failed", error);
    });
  }, [view]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!isCloseShortcut(event)) return;
      event.preventDefault();
      getApi()
        .packetInspectorClose()
        .catch((error: unknown) => {
          console.error("Closing the packet inspector failed", error);
        });
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  const names = useMemo(() => namesOf(view), [view]);

  return (
    <main className="pi-window" aria-labelledby={titleId}>
      <h1 id={titleId} className="settings-section__title">
        {t("sections.packetInspector")}
      </h1>
      <p className="settings-hint">{t("inspector.intro")}</p>
      <PacketInspector names={names} />
    </main>
  );
}
