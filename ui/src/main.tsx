import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./theme/global.css";
import { initialLanguage } from "./i18n";
import { App } from "./app/App";
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

// The first frame is already in the OS language; the stored setting follows once settings load.
void initialLanguage.then(() => {
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
});
