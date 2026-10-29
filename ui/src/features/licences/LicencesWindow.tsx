import { useEffect, useState } from "react";
import { LiveRegions } from "../../a11y";
import { getApi, type WindowLook } from "../../api";
import { applyLanguage, resolveLanguage } from "../../i18n";
import { applyRootLook, isCloseShortcut } from "../inspector/inspectorView";
import { LicencesView } from "./LicencesView";
import { loadNotices, type Notices } from "./notices";

/**
 * Root of the third-party licences window (issue #138). It takes theme and language from the
 * main window, which shares them through the engine; this window cannot read the settings.
 */
export function LicencesWindow() {
  const [look, setLook] = useState<WindowLook | null>(null);
  const [notices, setNotices] = useState<Notices | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const api = getApi();
    let updated = false;
    const stop = api.on("windowLookChanged", (changed) => {
      updated = true;
      setLook(changed.look);
    });
    api.windowLook().then(
      (shared) => {
        if (!updated && shared !== null) setLook(shared);
      },
      (error: unknown) => {
        console.error("Loading the main window's look failed", error);
      },
    );
    return stop;
  }, []);

  useEffect(() => {
    if (look === null) return;
    applyRootLook(document.documentElement, look);
    applyLanguage(resolveLanguage(null, [look.language])).catch((error: unknown) => {
      console.error("Switching the UI language failed", error);
    });
  }, [look]);

  useEffect(() => {
    let active = true;
    loadNotices().then(
      (loaded) => {
        if (active) setNotices(loaded);
      },
      (error: unknown) => {
        console.error("Loading the third-party licences failed", error);
        if (active) setFailed(true);
      },
    );
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!isCloseShortcut(event)) return;
      event.preventDefault();
      getApi()
        .licencesClose()
        .catch((error: unknown) => {
          console.error("Closing the licences window failed", error);
        });
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  return (
    <>
      <LicencesView notices={notices} failed={failed} />
      <LiveRegions />
    </>
  );
}
