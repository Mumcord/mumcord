import { describe, expect, it } from "vitest";
import { commandName, parseDuration, reasonAfterName, slashQuery } from "./parse";

describe("splitting a typed command (FR-10.30)", () => {
  it("names the command and marks the word that is still being written", () => {
    expect(slashQuery("/")).toEqual({ name: "", words: [], partial: "", naming: true });
    expect(slashQuery("/kic")).toEqual({ name: "kic", words: [], partial: "", naming: true });
    expect(slashQuery("/kick Ma")).toEqual({
      name: "kick",
      words: [],
      partial: "Ma",
      naming: false,
    });
    // A trailing space means the word is finished and the next one starts.
    expect(slashQuery("/kick ")).toEqual({ name: "kick", words: [], partial: "", naming: false });
    expect(slashQuery("/move Freya A")).toEqual({
      name: "move",
      words: ["Freya"],
      partial: "A",
      naming: false,
    });
    expect(slashQuery("/move Freya ")).toEqual({
      name: "move",
      words: ["Freya"],
      partial: "",
      naming: false,
    });
  });

  it("does not count a double space as an extra argument", () => {
    expect(slashQuery("/move Freya  ")).toEqual({
      name: "move",
      words: ["Freya"],
      partial: "",
      naming: false,
    });
    expect(slashQuery("/move  Freya A")).toEqual({
      name: "move",
      words: ["Freya"],
      partial: "A",
      naming: false,
    });
  });

  it("keeps the spaces of a reason, which is one text and not several words", () => {
    const query = slashQuery("/kick Mallory spam links");
    expect(query?.words).toEqual(["Mallory", "spam"]);
    expect(query?.partial).toBe("links");
    expect(reasonAfterName(["spam", "links"], "")).toBe("spam links");
  });

  it("is no command at all for plain text or a name the keyboard does not make", () => {
    expect(slashQuery("hello")).toBeNull();
    expect(slashQuery("/")).not.toBeNull();
    expect(slashQuery("/kick!")).toBeNull();
    expect(commandName("/ban Mallory")).toBe("ban");
    expect(commandName("/ban")).toBe("ban");
    expect(commandName("plain /ban")).toBeNull();
  });
});

describe("the duration of /ban (FR-10.31)", () => {
  it("reads hours, days and permanent as the dialog's chips", () => {
    expect(parseDuration("2h")).toBe(7_200);
    expect(parseDuration("7d")).toBe(604_800);
    expect(parseDuration("PERM")).toBe(0);
    expect(parseDuration("permanent")).toBe(0);
  });

  it("is no duration when the word is already the reason", () => {
    expect(parseDuration("spam")).toBeNull();
    expect(parseDuration(undefined)).toBeNull();
    expect(parseDuration("0d")).toBeNull();
  });
});
