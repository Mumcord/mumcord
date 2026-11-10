/**
 * The `mumcord://plugins/<id>` links of a message (FR-6.65a). The parsing rule matches
 * `parse_plugin_link` in `src-tauri/src/deeplink.rs`: the link is untrusted input, so only the
 * plugin id is taken from it, and everything the card shows comes from the plugin index.
 */

/** More cards than this per message would bury the conversation (the same cap as the previews). */
const MAX_LINKS = 3;

/** Plugin ids follow FR-6.11, like `is_plugin_id` in the app's link handler. */
const PLUGIN_ID = /^[a-z][a-z0-9-]{2,31}$/;

const PLUGIN_HREF = /^mumcord:\/\//i;
const PLUGIN_TEXT = /mumcord:\/\/[^\s<>"']+/gi;
/** Punctuation that ends the sentence around a typed link, not the link itself. */
const TRAILING_PUNCTUATION = /[.,;:!?)\]]+$/;

/** The plugin id of a `mumcord://plugins/<id>` link; `null` for any other link. */
export function parsePluginLink(url: string): string | null {
  let link: URL;
  try {
    link = new URL(url);
  } catch {
    return null;
  }
  if (link.protocol !== "mumcord:" || link.hostname !== "plugins") return null;
  const id = link.pathname.startsWith("/") ? link.pathname.slice(1) : null;
  return id !== null && PLUGIN_ID.test(id) ? id : null;
}

/** The link of a plugin in its plain form; the key its card is shown and dismissed under. */
export function pluginLink(id: string): string {
  return `mumcord://plugins/${id}`;
}

/**
 * The plugin links of a message in their plain form, in document order, each plugin once and at
 * most three. A link can arrive as an anchor or as text the user typed. The engine's own HTML is
 * read here, because the UI sanitizer drops `mumcord:` targets.
 */
export function pluginLinks(html: string): string[] {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const links: string[] = [];
  const add = (url: string) => {
    const id = parsePluginLink(url);
    if (id === null || links.length === MAX_LINKS || links.includes(pluginLink(id))) return;
    links.push(pluginLink(id));
  };
  const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    if (node instanceof HTMLAnchorElement) {
      const href = node.getAttribute("href")?.trim() ?? "";
      if (PLUGIN_HREF.test(href)) add(href);
    } else if (node.nodeType === Node.TEXT_NODE) {
      for (const match of (node.textContent ?? "").matchAll(PLUGIN_TEXT)) {
        add(match[0].replace(TRAILING_PUNCTUATION, ""));
      }
    }
  }
  return links;
}
