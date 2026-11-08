/**
 * The built-in admin commands of the chat box (Epic 4, issue #378, FR-10.30). What each one needs
 * is decided here: the right that lists it at all, the people it may name and the shape of its
 * arguments. Running one is in `run.ts`, offering them while typing in `useAdminSlash.ts`.
 */
import type { User } from "../../../api";
import type { ServerState } from "../../channels/serverStore";
import {
  mayBan,
  mayKick,
  mayMove,
  mayMoveAnywhere,
  mayMute,
  mayMuteAnywhere,
  mayRegister,
} from "../permissions";

/** The kinds of argument a command takes; each is completed by a name the admin may use (FR-10.32). */
export type ArgKind = "person" | "channel" | "ban";

/** Every built-in command name; the texts of the UI are keyed by these (FR-10.30). */
export type AdminCommandName =
  | "mute"
  | "unmute"
  | "deaf"
  | "undeaf"
  | "priority"
  | "register"
  | "move"
  | "moveall"
  | "unban"
  | "kick"
  | "ban";

export interface AdminCommand {
  readonly name: AdminCommandName;
  /** The arguments in order; the free text after the last one (a reason) is not listed. */
  readonly args: readonly ArgKind[];
  /** Listed and autocompleted only when the server's answers grant the right (FR-10.32). */
  readonly allowed: (state: ServerState) => boolean;
  /** Whether this command may name a person (FR-10.32: only the ones the admin may act on). */
  readonly mayName?: (state: ServerState, user: User) => boolean;
  /** `/register` only offers guests, because a registered person cannot be registered again. */
  readonly guestsOnly?: boolean;
}

/** Server mute, deafen and priority speaker all need Mute others in the person's channel. */
const mayMutePerson = (state: ServerState, user: User): boolean => mayMute(state, user.channelId);
/** Moving one person needs the Move right in the channel they are in. */
const mayMovePerson = (state: ServerState, user: User): boolean => mayMove(state, user.channelId);

/** The order of the list: toggles, then move, then the ones that confirm, then the red ones. */
export const ADMIN_COMMANDS: readonly AdminCommand[] = [
  {
    name: "mute",
    args: ["person"],
    allowed: mayMuteAnywhere,
    mayName: mayMutePerson,
  },
  {
    name: "unmute",
    args: ["person"],
    allowed: mayMuteAnywhere,
    mayName: mayMutePerson,
  },
  {
    name: "deaf",
    args: ["person"],
    allowed: mayMuteAnywhere,
    mayName: mayMutePerson,
  },
  {
    name: "undeaf",
    args: ["person"],
    allowed: mayMuteAnywhere,
    mayName: mayMutePerson,
  },
  {
    name: "priority",
    args: ["person"],
    allowed: mayMuteAnywhere,
    mayName: mayMutePerson,
  },
  {
    name: "register",
    args: ["person"],
    allowed: mayRegister,
    guestsOnly: true,
  },
  {
    name: "move",
    args: ["person", "channel"],
    allowed: mayMoveAnywhere,
    mayName: mayMovePerson,
  },
  {
    name: "moveall",
    args: ["channel"],
    allowed: mayMoveAnywhere,
  },
  {
    name: "unban",
    args: ["ban"],
    allowed: mayBan,
  },
  {
    name: "kick",
    args: ["person"],
    allowed: mayKick,
  },
  {
    name: "ban",
    args: ["person"],
    allowed: mayBan,
  },
] as const satisfies readonly AdminCommand[];

/** Every built-in name, permitted or not: one of them never goes into the chat (FR-10.33). */
export const BUILT_IN_NAMES: ReadonlySet<string> = new Set(
  ADMIN_COMMANDS.map((command) => command.name),
);

/** A built-in command by its typed name. */
export function adminCommand(name: string): AdminCommand | undefined {
  return ADMIN_COMMANDS.find((command) => command.name === name);
}

/** The commands this server's rights allow, in list order (FR-10.32). */
export function allowedCommands(state: ServerState): AdminCommand[] {
  return ADMIN_COMMANDS.filter((command) => command.allowed(state));
}
