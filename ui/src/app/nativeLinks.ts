// A middle click on a link, or a click with Ctrl, Cmd or Shift held, is the webview's "open this
// somewhere else": it loads the page in a window of its own or navigates the app away, so a
// Mumcord window can end up showing an arbitrary website (issue #418). The webview never gets the
// chance: the link goes to the system browser, exactly like a plain click. Installed for the whole
// page, so every view (chat, What's new, the licences, the pop-outs) is covered.
import { openInBrowser } from "../features/chat/openLink";

/** The link an activation happened on, if any. */
function linkHref(event: MouseEvent): string | null {
  const link = event.target instanceof Element ? event.target.closest("a[href]") : null;
  return link?.getAttribute("href") ?? null;
}

/**
 * Guards the page against link activations that would leave it. A view that handles the click
 * itself (the chat, "What's new") has already called `preventDefault`, so its link menu or its
 * own opener stays in charge and the link does not open twice.
 */
export function installLinkGuard(
  doc: Document = document,
  open: (url: string) => void = openInBrowser,
): () => void {
  const onAuxClick = (event: MouseEvent) => {
    if (event.button !== 1) return;
    const href = linkHref(event);
    if (href === null) return;
    event.preventDefault();
    open(href);
  };
  const onClick = (event: MouseEvent) => {
    if (!(event.ctrlKey || event.metaKey || event.shiftKey) || event.defaultPrevented) return;
    const href = linkHref(event);
    if (href === null) return;
    event.preventDefault();
    open(href);
  };
  doc.addEventListener("auxclick", onAuxClick);
  doc.addEventListener("click", onClick);
  return () => {
    doc.removeEventListener("auxclick", onAuxClick);
    doc.removeEventListener("click", onClick);
  };
}
