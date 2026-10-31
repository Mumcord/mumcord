/**
 * Turns `@Name` in sanitized message HTML into focusable chips for people who are on the server
 * (FR-1.51), and `@all` / `@channel` into plain highlighted chips (issue #70). Works on text nodes only and builds elements with the DOM API, so no message text is
 * ever parsed as markup; code blocks and links are left alone.
 */
import type { User } from "../../api";
import { foldName, GROUP_MENTIONS, isWholeMention } from "./mentions";

export const MENTION_CLASS = "chat-mention";
/** `@all` / `@channel`: highlighted like a person's chip, but nothing to open. */
export const GROUP_MENTION_CLASS = "chat-mention-group";
const SKIP = "a, pre, code, script, style";

export interface MentionPerson {
  session: number;
  name: string;
}

interface Folded {
  text: string;
  /** Original index of each folded character. */
  origin: number[];
}

function fold(text: string): Folded {
  let folded = "";
  const origin: number[] = [];
  let index = 0;
  for (const char of text) {
    const part = foldName(char);
    folded += part;
    for (let i = 0; i < part.length; i += 1) origin.push(index);
    index += char.length;
  }
  origin.push(index);
  return { text: folded, origin };
}

/** People with a usable name, longest name first so "@Anna Maria" wins over "@Anna". */
export function mentionPeople(users: Iterable<User>): MentionPerson[] {
  const people: (MentionPerson & { folded: string })[] = [];
  for (const user of users) {
    const folded = foldName(user.name.trim());
    if (folded !== "") people.push({ session: user.session, name: user.name, folded });
  }
  people.sort((a, b) => b.folded.length - a.folded.length || a.session - b.session);
  return people;
}

const peopleByMap = new WeakMap<ReadonlyMap<number, User>, MentionPerson[]>();

/**
 * `mentionPeople` of a server's people map, worked out once per map: every message body needs
 * it, and sorting a big server's people again for each of hundreds of messages froze the window.
 */
export function mentionPeopleOf(users: ReadonlyMap<number, User>): MentionPerson[] {
  let people = peopleByMap.get(users);
  if (people === undefined) {
    people = mentionPeople(users.values());
    peopleByMap.set(users, people);
  }
  return people;
}

function chip(doc: Document, text: string, person: MentionPerson, label: string): HTMLElement {
  const element = doc.createElement("span");
  element.className = MENTION_CLASS;
  element.setAttribute("role", "button");
  element.setAttribute("tabindex", "-1");
  element.setAttribute("aria-label", label);
  element.dataset["mentionSession"] = String(person.session);
  element.dataset["mentionName"] = person.name;
  element.textContent = text;
  return element;
}

function groupChip(doc: Document, text: string): HTMLElement {
  const element = doc.createElement("span");
  element.className = GROUP_MENTION_CLASS;
  element.textContent = text;
  return element;
}

/** The group keyword of a whole-word `@all` / `@channel` at `at` in folded `text`. */
function groupAt(text: string, at: number): string | undefined {
  return GROUP_MENTIONS.find(
    (group) => text.startsWith(group, at + 1) && isWholeMention(text, at, at + 1 + group.length),
  );
}

function splitText(
  doc: Document,
  node: Text,
  people: readonly (MentionPerson & { folded: string })[],
  label: (name: string) => string,
): void {
  const source = node.data;
  if (!source.includes("@")) return;
  const { text, origin } = fold(source);
  const parts: (string | HTMLElement)[] = [];
  let copied = 0;
  for (let at = text.indexOf("@"); at >= 0; at = text.indexOf("@", at + 1)) {
    const person = people.find((p) => {
      const end = at + 1 + p.folded.length;
      return text.startsWith(p.folded, at + 1) && isWholeMention(text, at, end);
    });
    // A person who is really called "all" keeps their own chip.
    const keyword = person === undefined ? groupAt(text, at) : person.folded;
    if (keyword === undefined) continue;
    const start = origin[at] ?? 0;
    const end = origin[at + 1 + keyword.length] ?? source.length;
    if (start < copied) continue;
    if (start > copied) parts.push(source.slice(copied, start));
    const shown = source.slice(start, end);
    parts.push(person ? chip(doc, shown, person, label(person.name)) : groupChip(doc, shown));
    copied = end;
  }
  if (parts.length === 0) return;
  if (copied < source.length) parts.push(source.slice(copied));
  node.replaceWith(...parts);
}

/**
 * `html` with a chip around every mention of one of `people` and every `@all` / `@channel`;
 * unchanged when there is none.
 */
export function linkMentions(
  html: string,
  people: readonly MentionPerson[],
  label: (name: string) => string,
): string {
  if (!html.includes("@")) return html;
  const withFolded = people.map((p) => ({ ...p, folded: foldName(p.name.trim()) }));
  const doc = new DOMParser().parseFromString(html, "text/html");
  const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (node instanceof Text && !node.parentElement?.closest(SKIP)) nodes.push(node);
  }
  for (const node of nodes) splitText(doc, node, withFolded, label);
  return doc.body.innerHTML;
}
