/**
 * State of the one shared person context menu (FR-1.18, QA.32). Person rows, chat names and
 * avatars report right-click / ContextMenu key / Shift+F10 here; `PersonMenuHost` renders it.
 */
import type { KeyboardEvent, MouseEvent } from "react";
import { closePreview } from "./profilePreviewStore";

export interface OpenPersonMenu {
  session: number;
  /** Viewport position of the pointer (or of the person for keyboard use). */
  x: number;
  y: number;
  /** Element that gets focus back when the menu closes. */
  opener: HTMLElement | null;
}

let current: OpenPersonMenu | null = null;
const listeners = new Set<() => void>();

function set(next: OpenPersonMenu | null) {
  current = next;
  for (const listener of listeners) listener();
}

export function subscribePersonMenu(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function currentPersonMenu(): OpenPersonMenu | null {
  return current;
}

/** Opens the menu (replacing any open one) and closes the hover preview. */
export function openPersonMenu(menu: OpenPersonMenu): void {
  closePreview();
  set(menu);
}

export function closePersonMenu(): void {
  if (current) set(null);
}

const FOCUSABLE = 'button, [tabindex]:not([tabindex="-1"]), [role="treeitem"]';

function openerOf(element: HTMLElement): HTMLElement {
  return element.matches(FOCUSABLE)
    ? element
    : (element.querySelector<HTMLElement>(FOCUSABLE) ?? element);
}

/** Right-click, or the ContextMenu key (browsers send `contextmenu` for it, without a pointer). */
export function openPersonMenuFromEvent(session: number, event: MouseEvent<HTMLElement>): void {
  event.preventDefault();
  const element = event.currentTarget;
  const opener = openerOf(element);
  const fromKeyboard = event.clientX === 0 && event.clientY === 0;
  const rect = (fromKeyboard ? opener : element).getBoundingClientRect();
  openPersonMenu({
    session,
    x: fromKeyboard ? rect.left : event.clientX,
    y: fromKeyboard ? rect.bottom : event.clientY,
    opener,
  });
}

/** Keyboard fallback for engines that don't turn ContextMenu / Shift+F10 into `contextmenu`. */
export function openPersonMenuFromKey(session: number, event: KeyboardEvent<HTMLElement>): void {
  if (event.key !== "ContextMenu" && !(event.key === "F10" && event.shiftKey)) return;
  event.preventDefault();
  const opener = event.target instanceof HTMLElement ? event.target : openerOf(event.currentTarget);
  const rect = opener.getBoundingClientRect();
  openPersonMenu({ session, x: rect.left, y: rect.bottom, opener });
}
