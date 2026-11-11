/**
 * Running one built-in admin command of the chat box (Epic 4, issue #378, FR-10.30 ff.): the words
 * are resolved to a person or a channel, the engine does the action, and every command answers with
 * a line only this admin sees (FR-10.35). Kick and ban are the exception — they open their dialog
 * prefilled and report from there (FR-10.30).
 */
import { getApi, toEngineError, type BanEntry, type PersonAction, type User } from "../../../api";
import type { SlashOption } from "../../chat/slashCommands";
import { selfUser, type ServerState } from "../../channels/serverStore";
import { actionErrorText } from "../errors";
import { openPersonAction } from "../personActionStore";
import { currentBanWindow } from "../banStore";
import { adminCommand, type AdminCommand } from "./commands";
import { say, type CommandContext } from "./context";
import { stageConfirm } from "./confirmStore";
import { commandName, parseDuration, slashQuery } from "./parse";
import {
  channelCandidates,
  personCandidates,
  resolveLeading,
  resolveOne,
  type Candidate,
} from "./resolve";

/** What the runner wants to show in or above the box (FR-10.34). */
export type RunResult =
  /** Ran, or opened a dialog that takes it from here. */
  | { kind: "done" }
  /** A line above the message box: nothing matched, or the server refused. */
  | { kind: "notice"; text: string }
  /** Several people or bans match; the list asks which one was meant (mockup `04c`). */
  | { kind: "choice"; options: SlashOption[] };

/** Whether this line names one of Mumcord's commands, permitted or not (FR-10.33). */
export function isBuiltIn(line: string): boolean {
  const name = commandName(line);
  return name !== null && adminCommand(name) !== undefined;
}

/**
 * Runs a built-in command. The caller has already accepted that the line names one; the rights are
 * checked again here, because the admin can type a command by hand that the list never showed
 * (FR-10.33): nothing happens and nothing is sent.
 */
export async function runBuiltIn(line: string, ctx: CommandContext): Promise<RunResult> {
  const query = slashQuery(line);
  if (query === null) return { kind: "done" };
  const command = adminCommand(query.name);
  if (command === undefined || !command.allowed(ctx.state)) return { kind: "done" };
  const tokens = query.partial === "" ? query.words : [...query.words, query.partial];
  return run(command, tokens, ctx);
}

async function run(
  command: AdminCommand,
  tokens: readonly string[],
  ctx: CommandContext,
): Promise<RunResult> {
  const api = getApi();
  switch (command.name) {
    case "kick":
      return withPerson(command, tokens, ctx, (person) => {
        openPersonAction(ctx.serverId, person.user.session, person.name, person.channel, "kick", {
          reason: person.rest,
          conversation: ctx.conversation,
        });
        return Promise.resolve({ kind: "done" } satisfies RunResult);
      });
    case "ban":
      return withPerson(command, tokens, ctx, (person) => {
        // `[duration]` preselects its chip; without it the dialog opens with "Permanent" (FR-10.31).
        const duration = parseDuration(person.words[0]);
        openPersonAction(ctx.serverId, person.user.session, person.name, person.channel, "ban", {
          reason: duration === null ? person.rest : person.words.slice(1).join(" ").trim(),
          duration: duration ?? 0,
          conversation: ctx.conversation,
        });
        return Promise.resolve({ kind: "done" } satisfies RunResult);
      });
    case "mute":
      return withPerson(command, tokens, ctx, (person) =>
        act(
          ctx,
          () => api.personSetServerMute(ctx.serverId, person.user.session, true),
          () => ctx.t("commands.results.muted", { name: person.name }),
        ),
      );
    case "unmute":
      return withPerson(command, tokens, ctx, (person) =>
        act(
          ctx,
          () => api.personSetServerMute(ctx.serverId, person.user.session, false),
          () => ctx.t("commands.results.unmuted", { name: person.name }),
        ),
      );
    case "deaf":
      return withPerson(command, tokens, ctx, (person) =>
        act(
          ctx,
          () => api.personSetServerDeafen(ctx.serverId, person.user.session, true),
          () => ctx.t("commands.results.deafened", { name: person.name }),
        ),
      );
    case "undeaf":
      return withPerson(command, tokens, ctx, (person) =>
        act(
          ctx,
          () => api.personSetServerDeafen(ctx.serverId, person.user.session, false),
          () => ctx.t("commands.results.undeafened", { name: person.name }),
        ),
      );
    case "priority":
      // A toggle, so the tree decides which way it goes (FR-10.30).
      return withPerson(command, tokens, ctx, (person) => {
        const on = !person.user.prioritySpeaker;
        return act(
          ctx,
          () => api.personSetPrioritySpeaker(ctx.serverId, person.user.session, on),
          () =>
            on
              ? ctx.t("commands.results.priorityOn", { name: person.name })
              : ctx.t("commands.results.priorityOff", { name: person.name }),
        );
      });
    case "register":
      return withPerson(command, tokens, ctx, (person) =>
        act(
          ctx,
          () => api.personRegister(ctx.serverId, person.user.session),
          () => ctx.t("commands.results.registered", { name: person.name }),
        ),
      );
    case "move":
      return withPerson(command, tokens, ctx, (person) => {
        // The channel may hold spaces ("Raid 2"), so the whole rest of the line names it.
        const target = channel(person.rest, ctx);
        if (target.kind !== "found") return Promise.resolve(target);
        return act(
          ctx,
          () => api.personMove(ctx.serverId, person.user.session, target.id),
          () => ctx.t("commands.results.moved", { name: person.name, channel: target.name }),
        );
      });
    case "moveall":
      return withChannel(tokens.join(" "), ctx, (target) => {
        const from = ownChannel(ctx);
        if (from === undefined) return Promise.resolve({ kind: "done" } as RunResult);
        // Moving a channel into itself has nothing to move, so the admin is told instead of being
        // asked and then answered "0 of 3 people; 3 were refused" (FR-10.17, FR-10.35).
        if (from.id === target.id) {
          return Promise.resolve({
            kind: "notice",
            text: ctx.t("commands.errors.alreadyInChannel", { channel: from.name }),
          } satisfies RunResult);
        }
        // The question carries the count and both names (FR-10.17, FR-10.56).
        stageConfirm({
          kind: "moveAll",
          serverId: ctx.serverId,
          conversation: ctx.conversation,
          fromId: from.id,
          from: from.name,
          toId: target.id,
          to: target.name,
          count: [...ctx.state.users.values()].filter((user) => user.channelId === from.id).length,
        });
        return Promise.resolve({ kind: "done" } as RunResult);
      });
    case "unban":
      return withBan(tokens.join(" "), ctx);
    default:
      return { kind: "done" };
  }
}

/** The person an argument names, with the words that came after it. */
interface PersonArgument {
  user: User;
  name: string;
  /** The channel as the dialog's person line shows it (FR-10.57). */
  channel: string;
  words: readonly string[];
  rest: string;
}

/** Names the person, then runs the action; no name, no match or several matches say so (FR-10.34). */
async function withPerson(
  command: AdminCommand,
  tokens: readonly string[],
  ctx: CommandContext,
  act_: (person: PersonArgument) => Promise<RunResult>,
): Promise<RunResult> {
  const wanted = tokens[0];
  if (wanted === undefined || wanted === "") return { kind: "notice", text: usage(command, ctx) };
  const matches = peopleOf(command, ctx.state);
  // Names may hold spaces, so the longest run of words that is a full name is the person.
  const { found, used } = resolveLeading(tokens, matches);
  const rest = tokens.slice(used);
  if (found.kind === "many") {
    return {
      kind: "choice",
      options: choiceOptions(found.items, ctx, (item) => act_(personOf(item, rest, ctx))),
    };
  }
  if (found.kind === "none") {
    return {
      kind: "notice",
      text:
        found.suggestion === null
          ? ctx.t("commands.errors.unknownPerson", { name: wanted })
          : ctx.t("commands.errors.unknownPersonDidYouMean", {
              name: wanted,
              suggestion: found.suggestion,
            }),
    };
  }
  return act_(personOf(found.item, rest, ctx));
}

function personOf(user: User, words: readonly string[], ctx: CommandContext): PersonArgument {
  return {
    user,
    name: user.name,
    channel: ctx.state.channels.get(user.channelId)?.name ?? "",
    words,
    rest: words.join(" ").trim(),
  };
}

/** The people this command may name: online, not the admin, and only those it may act on (FR-10.32). */
export function peopleOf(command: AdminCommand, state: ServerState): Candidate<User>[] {
  const self = selfUser(state)?.session ?? null;
  return personCandidates(
    state.users.values(),
    self,
    (user) =>
      (command.mayName?.(state, user) ?? true) && !(command.guestsOnly === true && user.registered),
    (channelId) => state.channels.get(channelId)?.name ?? "",
  );
}

async function withChannel(
  wanted: string,
  ctx: CommandContext,
  act_: (channel: { id: number; name: string }) => Promise<RunResult>,
): Promise<RunResult> {
  const found = channel(wanted, ctx);
  if (found.kind !== "found") return found;
  return act_(found);
}

/** A named channel, or the reason nothing fits (FR-10.34). */
type ChannelArgument =
  { kind: "found"; id: number; name: string } | { kind: "notice"; text: string };

/** Finds the channel an argument names, or says why nothing fits. */
function channel(wanted: string, ctx: CommandContext): ChannelArgument {
  if (wanted === "") return { kind: "notice", text: ctx.t("commands.errors.needChannel") };
  const matches = channelCandidates(ctx.state.channels.values(), (parentId) =>
    parentId === null ? "" : (ctx.state.channels.get(parentId)?.name ?? ""),
  );
  const found = resolveOne(wanted, matches);
  if (found.kind === "many") {
    return {
      kind: "notice",
      text: ctx.t("commands.errors.ambiguousChannel", {
        names: found.items.map((item) => item.name).join(", "),
      }),
    };
  }
  if (found.kind === "none") {
    return { kind: "notice", text: ctx.t("commands.errors.unknownChannel", { name: wanted }) };
  }
  return { kind: "found", id: found.item.id, name: found.item.name };
}

/** The channel the admin is in, which `/moveall` empties (FR-10.17). */
function ownChannel(ctx: CommandContext): { id: number; name: string } | undefined {
  const own = selfUser(ctx.state);
  const channel = own === undefined ? undefined : ctx.state.channels.get(own.channelId);
  return channel === undefined ? undefined : { id: channel.id, name: channel.name };
}

/**
 * `/unban` reads the server's list to find the entry, then asks before anything is removed
 * (FR-10.55). The list stays in memory only while something uses it (FR-10.63), so it is closed
 * again — unless the Server administration window keeps it open for itself.
 */
async function withBan(wanted: string, ctx: CommandContext): Promise<RunResult> {
  if (wanted === "") return { kind: "notice", text: ctx.t("commands.errors.needBan") };
  const api = getApi();
  let entries: BanEntry[];
  try {
    entries = await api.banListOpen(ctx.serverId);
  } catch (error) {
    return { kind: "notice", text: actionErrorText(ctx.t, toEngineError(error)) };
  }
  try {
    const matches = banCandidates(entries);
    // The removal is confirmed afterwards, so a single partial match may be offered directly.
    const found = resolveOne(wanted, matches, true);
    if (found.kind === "many") {
      return {
        kind: "choice",
        options: choiceOptions(found.items, ctx, (entry) => {
          stageConfirm({
            kind: "unban",
            serverId: ctx.serverId,
            conversation: ctx.conversation,
            entry,
          });
          return Promise.resolve({ kind: "done" } as RunResult);
        }),
      };
    }
    if (found.kind === "none") {
      return { kind: "notice", text: ctx.t("commands.errors.unknownBan", { name: wanted }) };
    }
    stageConfirm({
      kind: "unban",
      serverId: ctx.serverId,
      conversation: ctx.conversation,
      entry: found.item,
    });
    return { kind: "done" };
  } finally {
    if (currentBanWindow()?.serverId !== ctx.serverId) void api.banListClose(ctx.serverId);
  }
}

function banCandidates(entries: readonly BanEntry[]): Candidate<BanEntry>[] {
  return entries.map((entry) => ({
    name: entry.name ?? entry.address,
    hint: entry.address,
    item: entry,
  }));
}

/**
 * Rows for an argument that several people or bans match: choosing one runs the command for it,
 * which is the whole point of the list (mockup `04c-error-ambiguous`).
 */
function choiceOptions<T>(
  items: readonly Candidate<T>[],
  ctx: CommandContext,
  runFor: (item: T) => Promise<RunResult>,
): SlashOption[] {
  return items.map((item, index) => ({
    id: `choice-${String(index)}-${item.name}`,
    group: ctx.t("commands.groupChoice"),
    keyword: item.name,
    hint: item.hint,
    tag: "",
    insert: item.name,
    run: () => {
      void runFor(item.item);
    },
  }));
}

/** Sends one change and reports what the server made of it (FR-10.35, FR-10.59). */
async function act(
  ctx: CommandContext,
  call: () => Promise<PersonAction>,
  text: () => string,
): Promise<RunResult> {
  try {
    const outcome = await call();
    // A server that never answered did not confirm the change, so the line says just that.
    say(ctx, outcome === "confirmed" ? text() : ctx.t("commands.results.noAnswer"));
    return { kind: "done" };
  } catch (error) {
    return { kind: "notice", text: actionErrorText(ctx.t, toEngineError(error)) };
  }
}

/** What the command takes, shown when the admin typed no argument at all. */
function usage(command: AdminCommand, ctx: CommandContext): string {
  return ctx.t("commands.usageLine", {
    command: `/${command.name}`,
    usage: ctx.t(`commands.usage.${command.name}`),
  });
}
