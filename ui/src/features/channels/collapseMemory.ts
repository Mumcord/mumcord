/**
 * The user's expand/collapse choices per server for this app session (QA.27), so they survive the
 * channel tree being shown again (reconnect, switching servers) instead of resetting to the start.
 */
const remembered = new Map<number, ReadonlySet<number>>();

export function rememberedCollapsed(serverId: number | null): ReadonlySet<number> | undefined {
  return serverId === null ? undefined : remembered.get(serverId);
}

export function rememberCollapsed(serverId: number | null, collapsed: ReadonlySet<number>): void {
  if (serverId !== null) remembered.set(serverId, collapsed);
}

export function resetCollapseMemory(): void {
  remembered.clear();
}
