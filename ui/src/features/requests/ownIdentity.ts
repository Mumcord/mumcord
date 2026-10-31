import type { Identity, SavedServer } from "../../api";

/**
 * Certificate hash the user connects to `serverId` with: the server's own identity, else the
 * default (also when the chosen one was deleted, like the engine does, issue #148).
 */
export function ownCertHash(
  servers: readonly SavedServer[],
  identities: readonly Identity[],
  serverId: number,
): string | null {
  const identityId = servers.find((s) => s.id === serverId)?.identityId ?? null;
  const chosen = identityId === null ? undefined : identities.find((i) => i.id === identityId);
  return (chosen ?? identities.find((i) => i.isDefault))?.certHash ?? null;
}
