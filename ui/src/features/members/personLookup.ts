import type { User } from "../../api";

/**
 * The connected person behind a trigger. Chat messages carry the sender's session and name;
 * sessions are reused after someone leaves, so a different name means the sender is gone. In that
 * case a person with the same name still online is used, otherwise `undefined` (limited preview
 * and menu from the stored name only). Without a name (tree rows) the session alone decides.
 */
export function findPerson(
  users: ReadonlyMap<number, User>,
  session: number | null,
  name: string | undefined,
): User | undefined {
  const bySession = session === null ? undefined : users.get(session);
  if (name === undefined || bySession?.name === name) return bySession;
  const named = [...users.values()].filter((user) => user.name === name);
  return named.length === 1 ? named[0] : undefined;
}
