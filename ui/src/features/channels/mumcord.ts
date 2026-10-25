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
