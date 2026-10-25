/**
 * Where a GIF comes from (QA.52). Giphy and Tenor ask for a visible "via" credit on their
 * content; the label links to the original page. Mirrors the hosts the engine recognises in
 * `crates/engine/src/web/rendition.rs`.
 */
export interface GifSource {
  brand: "giphy" | "tenor";
  /** The page the credit links to. */
  href: string;
}

const GIPHY_ID = /^[A-Za-z0-9]{8,30}$/;

function hostIs(host: string, domain: string): boolean {
  return host === domain || host.endsWith(`.${domain}`);
}

/** The Giphy or Tenor source of an image or page link, or `null` for any other address. */
export function gifSource(link: string): GifSource | null {
  let url: URL;
  try {
    url = new URL(link);
  } catch {
    return null;
  }
  if (url.protocol !== "https:" && url.protocol !== "http:") return null;
  const host = url.hostname.toLowerCase();
  const segments = url.pathname.split("/").filter(Boolean);
  if (hostIs(host, "giphy.com")) {
    // Page: /gifs/<slug>-<id>. Media: media*.giphy.com/media/<id>/…, i.giphy.com/<id>.gif.
    let id: string | undefined;
    if (segments[0] === "gifs" && segments.length === 2) id = segments[1]?.split("-").pop();
    else if (host === "i.giphy.com") id = segments[0]?.replace(/\.[a-z]+$/i, "");
    else if (host.startsWith("media")) {
      const at = segments.indexOf("media");
      id = segments[at + 1];
      // Newer media paths are /media/v1.<token>/<id>/giphy.gif.
      if (id?.startsWith("v1.")) id = segments[at + 2];
    }
    if (id === undefined || !GIPHY_ID.test(id)) return null;
    return { brand: "giphy", href: `https://giphy.com/gifs/${id}` };
  }
  if (hostIs(host, "tenor.com")) {
    if (host === "tenor.com" || host === "www.tenor.com") {
      if (!segments.includes("view")) return null;
      return { brand: "tenor", href: url.href };
    }
    if (host.startsWith("media") || host === "c.tenor.com") {
      return { brand: "tenor", href: url.href };
    }
  }
  return null;
}
