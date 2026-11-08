import { describe, expect, it } from "vitest";
import { getServerState } from "../../channels/serverStore";
import { connectedMock, resetAll } from "../../channels/testUtils";
import { channelCandidates, closest, editDistance, personCandidates, resolveOne } from "./resolve";

interface Named {
  readonly label: string;
}

const candidate = (name: string, hint = ""): { name: string; hint: string; item: Named } => ({
  name,
  hint,
  item: { label: name },
});

const NAMES = ["Mallory", "Mallory Ann", "Anna", "Ben"].map((name) => candidate(name));

describe("what a typed argument names (FR-10.34)", () => {
  it("takes a full name as that person, whoever else contains it", () => {
    const found = resolveOne("Mallory", NAMES);
    expect(found.kind).toBe("one");
    if (found.kind === "one") expect(found.item.label).toBe("Mallory");
  });

  it("takes a unique beginning of a name", () => {
    const found = resolveOne("Be", NAMES);
    expect(found.kind).toBe("one");
    if (found.kind === "one") expect(found.item.label).toBe("Ben");
  });

  it("asks which one was meant when several fit (mockup 04c)", () => {
    const found = resolveOne("Mal", NAMES);
    expect(found.kind).toBe("many");
    if (found.kind === "many")
      expect(found.items.map((item) => item.name)).toEqual(["Mallory", "Mallory Ann"]);
  });

  it("says so when nothing fits, with the closest name when there is one (mockup 04b)", () => {
    const typo = resolveOne("Malory", NAMES);
    expect(typo.kind).toBe("none");
    if (typo.kind === "none") expect(typo.suggestion).toBe("Mallory");
    const far = resolveOne("Zoe", NAMES);
    expect(far.kind).toBe("none");
    if (far.kind === "none") expect(far.suggestion).toBeNull();
  });

  it("compares names without case", () => {
    const found = resolveOne("anna", NAMES);
    expect(found.kind).toBe("one");
  });
});

describe("the closest name to a misspelling", () => {
  it("only reaches over a small edit", () => {
    expect(closest("Malory", ["Mallory", "Ben"])).toBe("Mallory");
    expect(closest("Zoe", ["Mallory", "Ben"])).toBeNull();
  });

  it("counts the edits of a distance", () => {
    expect(editDistance("", "")).toBe(0);
    expect(editDistance("abc", "abc")).toBe(0);
    expect(editDistance("abc", "abd")).toBe(1);
    expect(editDistance("kitten", "sitting")).toBe(3);
  });
});

describe("the people and channels an argument may name (FR-10.32)", () => {
  it("leaves yourself out and keeps only the people the command may act on", async () => {
    await connectedMock({ bindStore: true });
    const state = getServerState();
    const people = personCandidates(
      state.users.values(),
      state.selfSession,
      (person) => person.channelId === 2,
      (channelId) => state.channels.get(channelId)?.name ?? "",
    );
    // The mock's own user sits in Lobby with Anna and Ben, Carla alone in Raid.
    expect(people.map((person) => `${person.name} ${person.hint}`)).toEqual(["Carla Raid"]);
    resetAll();
  });

  it("names a channel with its parent as the hint", async () => {
    await connectedMock({ bindStore: true });
    const state = getServerState();
    const channels = channelCandidates(state.channels.values(), (parentId) =>
      parentId === null ? "" : (state.channels.get(parentId)?.name ?? ""),
    );
    expect(channels.map((entry) => `${entry.name} \u2039 ${entry.hint}`)).toEqual([
      "Root \u2039 ",
      "Lobby \u2039 Root",
      "Raid \u2039 Root",
      "AFK \u2039 Root",
    ]);
    resetAll();
  });
});
