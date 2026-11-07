/**
 * The Permissions tab's plain-language model (Epic 4, issue #252): permission names and groups as
 * the grid shows them (FR-10.40), the built-in groups (FR-10.38), the grid's rows, and what counts
 * as an unsaved change. The evaluation itself runs in the core (FR-10.45).
 */
import type { AclGroupView, AclRowInput, AclRuleView, ChannelAclView } from "../../api";

/** Mumble permission bits. */
export const PERM = {
  write: 0x1,
  traverse: 0x2,
  enter: 0x4,
  speak: 0x8,
  muteDeafen: 0x10,
  move: 0x20,
  makeChannel: 0x40,
  linkChannel: 0x80,
  whisper: 0x100,
  textMessage: 0x200,
  makeTempChannel: 0x400,
  listen: 0x800,
  kick: 0x1_0000,
  ban: 0x2_0000,
  register: 0x4_0000,
  selfRegister: 0x8_0000,
  resetUserContent: 0x10_0000,
} as const;

export type PermName = keyof typeof PERM;

/** The grid's columns in their four groups; Server ones only in Root (FR-10.40). */
export const PERMISSION_GROUPS: readonly {
  id: string;
  perms: readonly PermName[];
  rootOnly?: boolean;
}[] = [
  { id: "basics", perms: ["enter", "traverse", "speak", "whisper", "textMessage", "listen"] },
  { id: "moderation", perms: ["muteDeafen", "move"] },
  { id: "channelAdmin", perms: ["write", "makeChannel", "makeTempChannel", "linkChannel"] },
  {
    id: "server",
    perms: ["kick", "ban", "register", "selfRegister", "resetUserContent"],
    rootOnly: true,
  },
];

export function permissionsFor(root: boolean): PermName[] {
  return PERMISSION_GROUPS.filter((g) => root || g.rootOnly !== true).flatMap((g) => [...g.perms]);
}

export function nameOfBit(bit: number): PermName | undefined {
  return (Object.keys(PERM) as PermName[]).find((name) => PERM[name] === bit);
}

/** Groups the server fills itself; listed read-only with an explanation (FR-10.38). */
export const BUILT_IN_GROUPS = ["all", "auth", "in", "out", "sub", "~", "#", "$"] as const;

export function isBuiltIn(name: string): boolean {
  const bare = name.replace(/^!/, "").replace(/^~/, "");
  return (
    ["all", "auth", "in", "out", "none", "strong"].includes(bare) ||
    bare.startsWith("sub") ||
    bare.startsWith("#") ||
    bare.startsWith("$")
  );
}

/** A rule's "Who", in words the UI shows next to its kind. */
export function ruleKind(rule: AclRuleView): "person" | "token" | "builtIn" | "group" {
  if (rule.userId !== null) return "person";
  const group = rule.group ?? "";
  if (group.replace(/^!/, "").startsWith("#")) return "token";
  return isBuiltIn(group) ? "builtIn" : "group";
}

export function userName(acl: ChannelAclView, id: number): string {
  return acl.userNames.find((u) => u.id === id)?.name ?? `#${String(id)}`;
}

export function ruleWho(acl: ChannelAclView, rule: AclRuleView): string {
  return rule.userId !== null ? userName(acl, rule.userId) : (rule.group ?? "");
}

/** The grid's rows: everyone, every group and person named in a rule, then the checked person. */
export function gridRows(
  acl: ChannelAclView,
  checked: AclRowInput | null,
): { key: string; row: AclRowInput }[] {
  const rows: { key: string; row: AclRowInput }[] = [
    { key: "everyone", row: { kind: "everyone" } },
  ];
  const seen = new Set(["everyone"]);
  for (const rule of acl.rules) {
    if (rule.userId !== null) {
      const key = `user:${String(rule.userId)}`;
      if (!seen.has(key)) rows.push({ key, row: { kind: "user", userId: rule.userId } });
      seen.add(key);
    } else if (rule.group !== null && rule.group !== "all") {
      const key = `group:${rule.group}`;
      if (!seen.has(key)) rows.push({ key, row: { kind: "group", name: rule.group } });
      seen.add(key);
    }
  }
  if (checked !== null) rows.push({ key: "checked", row: checked });
  return rows;
}

/** The part of the rules a save writes, to tell whether anything changed. */
function ownPart(acl: ChannelAclView): unknown {
  return {
    inheritAcls: acl.inheritAcls,
    rules: acl.rules.filter((r) => !r.inherited),
    groups: acl.groups.map((g) => ({ ...g, inheritedMembers: [] })),
  };
}

export function aclChanged(before: ChannelAclView, after: ChannelAclView): boolean {
  return JSON.stringify(ownPart(before)) !== JSON.stringify(ownPart(after));
}

/** Members of a group as this channel sees them (FR-10.38). */
export function members(group: AclGroupView): {
  id: number;
  state: "added" | "inherited" | "removed";
}[] {
  const inherited = group.inherit ? group.inheritedMembers : [];
  return [
    ...group.add.map((id) => ({ id, state: "added" as const })),
    ...inherited
      .filter((id) => !group.add.includes(id))
      .map((id) => ({
        id,
        state: group.remove.includes(id) ? ("removed" as const) : ("inherited" as const),
      })),
  ];
}

export function newRule(): AclRuleView {
  return {
    inherited: false,
    applyHere: true,
    applySubs: true,
    userId: null,
    group: "all",
    grant: 0,
    deny: 0,
  };
}
