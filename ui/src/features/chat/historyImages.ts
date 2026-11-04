/**
 * Images of synced messages that arrived as hash references (FR-1.60f). Another Mumcord user's
 * history sends each picture as a reference instead of its bytes; the engine fetches the image
 * and then updates the message. Until then the message shows a placeholder of the image's size.
 * The engine reports when an image cannot be loaded or waits for a manual load
 * ("Load full-size images from others" off); this store keeps those states for the hints.
 */
import { useSyncExternalStore } from "react";
import { getApi, type MumcordApi } from "../../api";
import type { HistoryImageState } from "../../bindings/commands";

/** Attribute of a referenced image in a synced body, written by the engine. */
export const IMAGE_REF_ATTRIBUTE = "data-mumcord-image";

/** `<sha256 hex>:<bytes>:<type>:<width>x<height>`, exactly as the engine writes it. */
export const IMAGE_REFERENCE =
  /^[0-9a-f]{64}:[1-9][0-9]{0,9}:(?:png|jpeg|gif|webp):([1-9][0-9]{0,3})x([1-9][0-9]{0,3})$/;

/** Class of the placeholder that stands in for a referenced image. */
export const PLACEHOLDER_CLASS = "history-image";

const states = new Map<string, HistoryImageState>();
const listeners = new Set<() => void>();
let snapshot: ReadonlyMap<string, HistoryImageState> = new Map();

function stateKey(serverId: number, reference: string): string {
  return `${String(serverId)} ${reference}`;
}

function changed(): void {
  snapshot = new Map(states);
  for (const listener of listeners) listener();
}

/** Records the engine's report about one placeholder. */
export function setHistoryImageState(
  serverId: number,
  reference: string,
  state: HistoryImageState,
): void {
  states.set(stateKey(serverId, reference), state);
  changed();
}

/** Listens to the engine's reports; returns the unsubscribe function. */
export function bindHistoryImages(api: MumcordApi): () => void {
  return api.on("historyImageStatus", ({ serverId, reference, state }) => {
    setHistoryImageState(serverId, reference, state);
  });
}

/** The reported state of each placeholder, by server and reference. */
export function useHistoryImageStates(): (
  serverId: number,
  reference: string,
) => HistoryImageState | undefined {
  const current = useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => snapshot,
  );
  return (serverId, reference) => current.get(stateKey(serverId, reference));
}

/** "Load image": asks the engine for the image; the message updates when it arrives. */
export function loadHistoryImage(serverId: number, reference: string): void {
  states.delete(stateKey(serverId, reference));
  changed();
  getApi()
    .historyImageLoad(serverId, reference)
    .catch((error: unknown) => {
      console.error("Loading the history image failed", error);
      setHistoryImageState(serverId, reference, "unavailable");
    });
}

/** The image references a message's HTML names, once each. */
export function historyImageReferences(html: string): string[] {
  const found = new Set<string>();
  for (const match of html.matchAll(new RegExp(`${IMAGE_REF_ATTRIBUTE}="([^"]*)"`, "g"))) {
    const reference = match[1];
    if (reference !== undefined && IMAGE_REFERENCE.test(reference)) found.add(reference);
  }
  return [...found];
}

/**
 * Turns the referenced images of sanitized HTML into placeholders of their size. Images without
 * a reference are untouched; a reference next to a real source is dropped.
 */
export function withPlaceholders(html: string): string {
  if (!html.includes(IMAGE_REF_ATTRIBUTE)) return html;
  const template = document.createElement("template");
  template.innerHTML = html;
  for (const img of template.content.querySelectorAll(`img[${IMAGE_REF_ATTRIBUTE}]`)) {
    const reference = img.getAttribute(IMAGE_REF_ATTRIBUTE) ?? "";
    const size = IMAGE_REFERENCE.exec(reference);
    if (img.hasAttribute("src") || size === null) {
      img.removeAttribute(IMAGE_REF_ATTRIBUTE);
      if (!img.hasAttribute("src")) img.remove();
      continue;
    }
    const [, width = "1", height = "1"] = size;
    const placeholder = document.createElement("span");
    placeholder.className = PLACEHOLDER_CLASS;
    placeholder.setAttribute(IMAGE_REF_ATTRIBUTE, reference);
    placeholder.style.width = `${width}px`;
    placeholder.style.aspectRatio = `${width} / ${height}`;
    img.replaceWith(placeholder);
  }
  return template.innerHTML;
}

/** Forgets every reported state (tests). */
export function resetHistoryImages(): void {
  states.clear();
  changed();
}
