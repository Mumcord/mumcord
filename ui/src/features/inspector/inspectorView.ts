/**
 * What the packet inspector window takes over from the main window (issue #60): the look of
 * the page (theme, effects, hint preferences, language) and the names the main window knows.
 * The inspector window has no access to settings or servers itself.
 */
import type { InspectorServer, InspectorUser, InspectorView } from "../../api";
import type { Names } from "./packets";

export type RootLook = Pick<InspectorView, "attributes" | "properties" | "language">;

/** `data-*` names as the DOM's `dataset` spells them. */
const ATTRIBUTE = /^[a-z][a-zA-Z0-9]*$/u;
/** CSS custom properties such as `--color-bg`. */
const PROPERTY = /^--[a-z0-9-]+$/u;

/** The look the main window's root element has right now. */
export function readRootLook(root: HTMLElement): RootLook {
  const attributes: Record<string, string> = {};
  for (const [key, value] of Object.entries(root.dataset)) {
    if (value !== undefined) attributes[key] = value;
  }
  const properties: Record<string, string> = {};
  for (let i = 0; i < root.style.length; i++) {
    const name = root.style.item(i);
    if (name.startsWith("--")) properties[name] = root.style.getPropertyValue(name);
  }
  return { attributes, properties, language: root.lang };
}

/** Gives `root` exactly the shared attributes and custom properties, dropping older ones. */
export function applyRootLook(
  root: HTMLElement,
  look: Pick<RootLook, "attributes" | "properties">,
) {
  for (const key of Object.keys(root.dataset)) {
    if (!(key in look.attributes)) Reflect.deleteProperty(root.dataset, key);
  }
  for (const [key, value] of Object.entries(look.attributes)) {
    if (ATTRIBUTE.test(key)) root.dataset[key] = value;
  }
  const stale: string[] = [];
  for (let i = 0; i < root.style.length; i++) {
    const name = root.style.item(i);
    if (name.startsWith("--") && !(name in look.properties)) stale.push(name);
  }
  for (const name of stale) root.style.removeProperty(name);
  for (const [name, value] of Object.entries(look.properties)) {
    if (PROPERTY.test(name)) root.style.setProperty(name, value);
  }
}

/** The shared names as the packet list looks them up; unknown until the main window shares. */
export function namesOf(view: InspectorView | null): Names {
  const users = new Map<string, string>();
  const servers = new Map<number, string>();
  for (const user of view?.users ?? []) {
    users.set(`${String(user.serverId)} ${String(user.session)}`, user.name);
  }
  for (const server of view?.servers ?? []) servers.set(server.serverId, server.label);
  return {
    user: (serverId, session) => users.get(`${String(serverId)} ${String(session)}`) ?? null,
    server: (serverId) => servers.get(serverId) ?? null,
  };
}

/** People of the connected servers, in a stable order so unchanged lists compare equal. */
export function sharedUsers(
  connected: ReadonlyMap<number, { users: ReadonlyMap<number, { name: string }> }>,
): InspectorUser[] {
  const users: InspectorUser[] = [];
  for (const [serverId, state] of connected) {
    for (const [session, user] of state.users) users.push({ serverId, session, name: user.name });
  }
  return users.sort((a, b) => a.serverId - b.serverId || a.session - b.session);
}

export function sharedServers(saved: readonly { id: number; label: string }[]): InspectorServer[] {
  return saved.map((server) => ({ serverId: server.id, label: server.label }));
}

/** Ctrl+W (⌘W on macOS) closes the inspector window; Esc deliberately does nothing there. */
export function isCloseShortcut(event: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey">) {
  return (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "w";
}
