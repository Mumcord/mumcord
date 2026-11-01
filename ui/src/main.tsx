import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./theme/global.css";
import { initialLanguage } from "./i18n";
import { App } from "./app/App";
import { ChangelogWindow, InspectorWindow, LicencesWindow, MediaPopoutWindow } from "./app/windows";
import { popoutKindOf } from "./features/media/popoutStore";
import { installNativeMenuSuppression } from "./app/nativeContextMenu";
import { installNativeDragGuard } from "./app/nativeDrag";
import { applyThemeToDocument, osPrefersDark, resolveTheme } from "./theme/theme";

const root = document.getElementById("root");
if (!root) {
  throw new Error("Root element #root is missing");
}

installNativeMenuSuppression();
installNativeDragGuard();

// Paint the OS theme right away; the stored theme setting replaces it once settings load.
applyThemeToDocument(resolveTheme("system", osPrefersDark()));

// The packet inspector (issue #60), the third-party licences (issue #138) and "What's new" (issue
// #145) have windows of their own that load the same page and render only their content, and so
// do the media pop-outs (issue #164).
const windowKind = new URLSearchParams(window.location.search).get("window");
const popoutKind = windowKind === "media-popout" ? popoutKindOf(window.location.search) : null;

// The first frame is already in the OS language; the stored setting follows once settings load.
void initialLanguage.then(() => {
  createRoot(root).render(
    <StrictMode>
      {windowKind === "packet-inspector" ? (
        <InspectorWindow />
      ) : windowKind === "licences" ? (
        <LicencesWindow />
      ) : windowKind === "changelog" ? (
        <ChangelogWindow />
      ) : popoutKind !== null ? (
        <MediaPopoutWindow kind={popoutKind} />
      ) : (
        <App />
      )}
    </StrictMode>,
  );
});
