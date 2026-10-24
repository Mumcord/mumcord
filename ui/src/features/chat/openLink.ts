/**
 * Opens a chat link in the system browser. Only called from a user's click, never
 * automatically, and only for http(s)/mailto targets.
 */
import { openUrl } from "@tauri-apps/plugin-opener";
import { SAFE_LINK } from "./sanitize";

export async function openLink(href: string): Promise<void> {
  if (!SAFE_LINK.test(href.trim())) return;
  if ("__TAURI_INTERNALS__" in window) {
    await openUrl(href);
  } else {
    // Browser preview with the mock engine (no Tauri): a new tab without access to this page.
    window.open(href, "_blank", "noopener,noreferrer");
  }
}
