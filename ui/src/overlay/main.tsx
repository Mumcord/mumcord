/** Entry of the overlay window (FR-1.44): a separate page with its own small React root. */
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../theme/global.css";
import { initialLanguage } from "../i18n";
import "./overlay.css";
import { OverlayApp } from "./OverlayApp";

const root = document.getElementById("root");
if (!root) {
  throw new Error("Root element #root is missing");
}

// The first frame is already in the OS language; the stored setting follows once settings load.
void initialLanguage.then(() => {
  createRoot(root).render(
    <StrictMode>
      <OverlayApp />
    </StrictMode>,
  );
});
