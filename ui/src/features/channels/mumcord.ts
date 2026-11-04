/** Helpers to recognise and gate on other Mumcord clients (FR-1.61). */
import type { User } from "../../api";

/** Whether the person is another Mumcord client. */
export function isMumcord(user: Pick<User, "isMumcord"> | undefined): boolean {
  return user?.isMumcord === true;
}

/** Whether the person is a Mumcord client that announced `feature`; gate Mumcord-only UI on this. */
export function hasMumcordFeature(
  user: Pick<User, "isMumcord" | "mumcordFeatures"> | undefined,
  feature: string,
): boolean {
  return isMumcord(user) && (user?.mumcordFeatures.includes(feature) ?? false);
}

/**
 * The software a bot's hello announces, e.g. "Mumbot 0.4.1" (issue #459). It wins over the Mumble
 * release string, which older bots fill with "Mumcord <version>". Null for people, who announce
 * no product. The engine has already limited both parts to plain letters, digits and dots.
 */
export function announcedClient(
  user: Pick<User, "bot" | "product" | "version"> | undefined,
): string | null {
  if (!user?.bot || !user.product) return null;
  return user.version ? `${user.product} ${user.version}` : user.product;
}
