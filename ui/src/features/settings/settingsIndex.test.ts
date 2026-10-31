import { describe, expect, it } from "vitest";
import de from "../../locales/de/settings.json";
import en from "../../locales/en/settings.json";
import es from "../../locales/es/settings.json";
import { SECTION_IDS } from "./sectionIds";
import {
  normalizeSearch,
  searchSettings,
  SETTINGS_INDEX,
  type SearchResult,
} from "./settingsIndex";

type Tree = { [key: string]: string | Tree };

function lookup(tree: Tree, key: string): string | undefined {
  let node: string | Tree | undefined = tree;
  for (const part of key.split(".")) {
    if (typeof node !== "object") return undefined;
    node = node[part];
  }
  return typeof node === "string" ? node : undefined;
}

const translator = (tree: Tree) => (key: string) => lookup(tree, key) ?? key;
const t = translator(en);
const labels = (results: SearchResult[]) =>
  results.map((result) =>
    result.kind === "section" ? result.section : (result.entry.control ?? result.entry.group),
  );

describe("settings search index", () => {
  it("only uses keys that exist in every language", () => {
    const keys = SETTINGS_INDEX.flatMap((entry) => [
      entry.group,
      entry.control,
      ...(entry.terms ?? []),
    ]).filter((key): key is string => key !== undefined);
    keys.push(...SECTION_IDS.map((id) => `sections.${id}`));
    for (const tree of [en, de, es] as Tree[]) {
      expect(keys.filter((key) => lookup(tree, key) === undefined)).toEqual([]);
    }
  });

  it("finds the language picker by any language's own name", () => {
    expect(labels(searchSettings("gaeilge", t))).toEqual(["language.legend"]);
    expect(labels(searchSettings("Ελληνικά", t))).toEqual(["language.legend"]);
    expect(labels(searchSettings("francais", t))).toEqual(["language.legend"]);
  });

  it("ignores case and accents", () => {
    expect(normalizeSearch("  Geräusch Música ")).toBe("gerausch musica");
  });

  it("finds sections by name and settings by label, group or hint", () => {
    expect(labels(searchSettings("voice", t))).toContain("voice");
    expect(labels(searchSettings("noise", t))).toEqual(
      expect.arrayContaining(["voice.noiseSuppression"]),
    );
    // "keyboards and fans" is only in the noise suppression hint.
    expect(labels(searchSettings("fans", t))).toEqual(["voice.noiseSuppression"]);
    expect(labels(searchSettings("skip links", t))).toEqual(["accessibility.skipLinks"]);
    expect(searchSettings("   ", t)).toEqual([]);
    expect(searchSettings("zzzz", t)).toEqual([]);
  });

  it("finds the newer settings rows", () => {
    expect(labels(searchSettings("connected servers", t))).toEqual(["servers.limit"]);
    expect(labels(searchSettings("hide empty", t))).toEqual(["channelList.hideEmpty"]);
    expect(labels(searchSettings("reduced transparency", t))).toEqual([
      "appearanceEffects.visualEffects",
    ]);
    expect(labels(searchSettings("allow direct connections", t))).toContain("privacyDirect.allow");
    expect(labels(searchSettings("download automatically", t))).toEqual(["updates.checking"]);
    expect(labels(searchSettings("advanced (ai)", t))).toEqual(["voice.noiseSuppression"]);
  });

  it("finds the history sharing switch (FR-1.60)", () => {
    expect(labels(searchSettings("share my channel history", t))).toEqual([
      "historySharing.switch",
    ]);
    expect(labels(searchSettings("private chats", t))).toContain("historySharing.switch");
    expect(labels(searchSettings("kanalverlauf", translator(de)))).toContain(
      "historySharing.switch",
    );
  });

  it("finds the full-size image switches (FR-1.74)", () => {
    expect(labels(searchSettings("offer my full-size", t))).toEqual(["fullImages.offer"]);
    expect(labels(searchSettings("never requested", t))).toEqual(["fullImages.load"]);
  });

  it("matches the current language without accents", () => {
    const results = labels(searchSettings("barrierefreiheit", translator(de)));
    expect(results).toContain("accessibility");
    expect(labels(searchSettings("musica", translator(es)))).toContain("audio.musicQuality");
  });
});
