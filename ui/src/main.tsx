import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./theme/global.css";
import "./i18n";
import { App } from "./app/App";
import { installNativeMenuSuppression } from "./app/nativeContextMenu";
import { applyThemeToDocument, osPrefersDark, resolveTheme } from "./theme/theme";

const root = document.getElementById("root");
if (!root) {
  throw new Error("Root element #root is missing");
}

installNativeMenuSuppression();

// Paint the OS theme right away; the stored theme setting replaces it once settings load.
applyThemeToDocument(resolveTheme("system", osPrefersDark()));

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
