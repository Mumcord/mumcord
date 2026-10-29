import { useEffect, useState } from "react";
import { LiveRegions } from "../../a11y";
import { getApi, type WindowLook } from "../../api";
import { applyLanguage, resolveLanguage } from "../../i18n";
import { applyRootLook, isCloseShortcut } from "../inspector/inspectorView";
import { loadChangelog, type ChangelogVersion } from "./changelog";
import { ChangelogView } from "./ChangelogView";

function close() {
  getApi()
    .changelogClose()
    .catch((error: unknown) => {
      console.error("Closing the changelog window failed", error);
    });
}

/**
 * Root of the "What's new" window (issue #145). Like the licences window it takes theme and
 * language from the main window, which shares them through the engine.
 */
export function ChangelogWindow() {
  const [look, setLook] = useState<WindowLook | null>(null);
  const [versions, setVersions] = useState<ChangelogVersion[] | null>(null);
  const [appVersion, setAppVersion] = useState<string | null>(null);
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
    Promise.all([loadChangelog(), getApi().appInfo()]).then(
      ([loaded, info]) => {
        if (!active) return;
        setVersions(loaded);
        setAppVersion(info.version);
      },
      (error: unknown) => {
        console.error("Loading the changelog failed", error);
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
      close();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  return (
    <>
      <ChangelogView versions={versions} appVersion={appVersion} failed={failed} onClose={close} />
      <LiveRegions />
    </>
  );
}
