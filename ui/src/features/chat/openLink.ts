/**
 * Opens a chat link in the system browser. Only called from a user's click, never
 * automatically, and only for http(s)/mailto targets. The engine starts the browser (with the
 * host environment inside an AppImage, issue #9); the mock engine opens a new tab.
 */
import { getApi } from "../../api";
import { SAFE_LINK } from "./sanitize";

export async function openLink(href: string): Promise<void> {
  const url = href.trim();
  if (!SAFE_LINK.test(url)) return;
  await getApi().openLink(url);
}

/** {@link openLink} for click handlers: failures are logged. */
export function openInBrowser(url: string): void {
  openLink(url).catch((error: unknown) => {
    console.error("Opening a link failed", error);
  });
}
