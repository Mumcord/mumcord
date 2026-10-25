/**
 * Links whose image is shown inline under their message (FR-1.55, QA.43). A message that is
 * only such a link shows the image instead of the address; the address stays reachable as the
 * image's "Open in browser" button and tooltip. Dismissing the image brings the text back.
 */
import { useSyncExternalStore } from "react";

const NONE: ReadonlySet<string> = new Set();
let shown = new Map<string, ReadonlySet<string>>();
const listeners = new Set<() => void>();

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function update(messageId: string, change: (urls: Set<string>) => void): void {
  const urls = new Set(shown.get(messageId) ?? NONE);
  change(urls);
  const next = new Map(shown);
  if (urls.size === 0) next.delete(messageId);
  else next.set(messageId, urls);
  shown = next;
  for (const listener of listeners) listener();
}

/** Marks the image of `url` as visible under the message; the returned function undoes it. */
export function markInlineShown(messageId: string, url: string): () => void {
  update(messageId, (urls) => urls.add(url));
  return () => {
    update(messageId, (urls) => urls.delete(url));
  };
}

export function useInlineShown(messageId: string): ReadonlySet<string> {
  return useSyncExternalStore(subscribe, () => shown.get(messageId) ?? NONE);
}

/** Forgets everything (tests). */
export function clearInlineShown(): void {
  shown = new Map();
  for (const listener of listeners) listener();
}

function isLineBreak(node: Node | null): boolean {
  return node instanceof Element && node.tagName === "BR";
}

function isBlank(node: Node | null): boolean {
  return node?.nodeType === Node.TEXT_NODE && (node.textContent ?? "").trim() === "";
}

/**
 * The sanitized message HTML without the links in `urls` whose image is shown. Only links whose
 * text is the address itself are hidden, and only when the message has nothing else, or when
 * the link is alone on the first or last line. In doubt the text stays.
 */
export function hideShownLinks(html: string, urls: ReadonlySet<string>): string {
  if (urls.size === 0) return html;
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, "text/html");
  const root = doc.body;
  const hideable = [...root.querySelectorAll("a[href]")].filter(
    (a) =>
      urls.has(a.getAttribute("href")?.trim() ?? "") &&
      a.textContent.trim() === a.getAttribute("href")?.trim(),
  );
  if (hideable.length === 0) return html;
  const onlyLinks = ((): boolean => {
    const copy = root.cloneNode(true) as HTMLElement;
    for (const a of copy.querySelectorAll("a[href]")) {
      if (hideable.some((h) => h.getAttribute("href") === a.getAttribute("href"))) a.remove();
    }
    for (const br of copy.querySelectorAll("br")) br.remove();
    return copy.textContent.trim() === "" && copy.querySelector("img, pre, code") === null;
  })();
  if (onlyLinks) {
    for (const a of hideable) a.remove();
    for (const br of root.querySelectorAll("br")) br.remove();
    return root.innerHTML;
  }
  const first = [...root.childNodes].find((n) => !isBlank(n)) ?? null;
  const last = [...root.childNodes].reverse().find((n) => !isBlank(n)) ?? null;
  if (first && hideable.includes(first as Element) && isLineBreak(first.nextSibling)) {
    first.nextSibling?.remove();
    first.remove();
  } else if (last && hideable.includes(last as Element) && isLineBreak(last.previousSibling)) {
    last.previousSibling?.remove();
    last.remove();
  }
  return root.innerHTML;
}
