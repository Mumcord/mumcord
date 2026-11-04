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

/** Whether the person is a bot by its hello (issue #458); the tester bot sends none and counts as a person. */
export function isBot(user: Pick<User, "bot"> | undefined): boolean {
  return user?.bot != null;
}

/**
 * A bot that never talks, e.g. the relay (issue #458). It is listed only under "Bots on this
 * server", never in the channel tree, the member list or any people picker.
 */
export function isHiddenBot(user: Pick<User, "bot"> | undefined): boolean {
  return user?.bot != null && !user.bot.voice;
}

/** Modules in the order the line names them: the bot's main job first. */
export const BOT_MODULES = ["peer", "music", "history", "webhook"];

/** The bots on the server, by name. */
export function serverBots(users: ReadonlyMap<number, User>): User[] {
  return [...users.values()]
    .filter((user) => isBot(user))
    .sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
}

/** Known modules first in `BOT_MODULES`, unknown ones after them alphabetically. */
export function orderedModules(modules: readonly string[]): string[] {
  const rank = (module: string) => {
    const index = BOT_MODULES.indexOf(module);
    return index === -1 ? BOT_MODULES.length : index;
  };
  return [...modules].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
}
