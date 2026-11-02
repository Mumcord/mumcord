import { useEffect, useState } from "react";
import { LiveRegions } from "../../a11y";
import { getApi, type WindowLook } from "../../api";
import { applyLanguage, resolveLanguage } from "../../i18n";
import { applyRootLook, isCloseShortcut } from "../inspector/inspectorView";
import { CHANGELOG_VERSIONS } from "./changelog";
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
    // No timeout: the webview can take 8-10 s to start (Linux AppImage), and an error shown while
    // the version is still loading would be wrong. Only a real failure is reported.
    getApi()
      .appInfo()
      .then(
        (info) => {
          if (!active) return;
          setAppVersion(info.version);
          setFailed(false);
        },
        (error: unknown) => {
          console.error("Loading the app version failed", error);
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
      <ChangelogView
        versions={appVersion === null ? null : CHANGELOG_VERSIONS}
        appVersion={appVersion}
        failed={failed}
        onClose={close}
      />
      <LiveRegions />
    </>
  );
}
