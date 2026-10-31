import { describe, expect, it } from "vitest";
import de from "../../locales/de/settings.json";
import en from "../../locales/en/settings.json";
import es from "../../locales/es/settings.json";
import { SECTION_IDS } from "./sectionIds";
import {
  groupResults,
  matchRanges,
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
  // English is the source; other languages fall back to it until they are translated.
  it("only uses keys that exist in English", () => {
    const keys = SETTINGS_INDEX.flatMap((entry) => [
      entry.group,
      entry.control,
      ...(entry.terms ?? []),
    ]).filter((key): key is string => key !== undefined);
    keys.push(...SECTION_IDS.flatMap((id) => [`sections.${id}`, `sectionDescriptions.${id}`]));
    expect(keys.filter((key) => lookup(en as Tree, key) === undefined)).toEqual([]);
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
    expect(labels(searchSettings("voice", t))).toContain("voiceAudio");
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
    expect(labels(searchSettings("github", t))).toEqual(["updates.check"]);
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
    expect(labels(searchSettings("sprunglinks", translator(de)))).toEqual([
      "accessibility.skipLinks",
    ]);
    expect(labels(searchSettings("musica", translator(es)))).toContain("audio.musicQuality");
  });

  it("finds settings by synonyms", () => {
    expect(labels(searchSettings("p2p", t))).toEqual(
      expect.arrayContaining(["p2p.title", "privacyDirect.allow"]),
    );
    expect(labels(searchSettings("hide my ip", t))).toContain("p2p.title");
    expect(labels(searchSettings("relay", t))).toContain("p2p.title");
  });

  it("files every setting under its new group", () => {
    const sectionOf = (control: string) =>
      SETTINGS_INDEX.find((entry) => entry.control === control)?.section;
    expect(sectionOf("voice.noiseSuppression")).toBe("voiceAudio");
    expect(sectionOf("audio.bitrate")).toBe("voiceAudio");
    expect(sectionOf("privacyDirect.allow")).toBe("privacy");
    expect(sectionOf("history.clearAll")).toBe("chatNotifications");
    expect(sectionOf("appearance.autostart")).toBe("general");
    expect(sectionOf("positional.switch")).toBe("gaming");
    expect(sectionOf("cache.clear")).toBe("advanced");
    expect(sectionOf("updates.check")).toBe("general");
  });

  it("leaves out the entries of hidden cards", () => {
    expect(labels(searchSettings("in-game overlay", t))).toContain("overlay.inGame.title");
    expect(labels(searchSettings("in-game overlay", t, ["overlay.inGame.title"]))).toEqual([]);
  });

  it("buckets results by group in the order of the side list", () => {
    const groups = groupResults(searchSettings("history", t)).map((group) => group.section);
    expect(groups).toEqual([...SECTION_IDS].filter((id) => groups.includes(id)));
    expect(groups).toEqual(expect.arrayContaining(["chatNotifications", "privacy", "servers"]));
  });

  it("marks matches ignoring case and accents", () => {
    expect(matchRanges("Noise suppression", "noise")).toEqual([[0, 5]]);
    expect(matchRanges("Gerät wählen", "gerat")).toEqual([[0, 5]]);
    expect(matchRanges("Echo cancellation", "can ella")).toEqual([
      [5, 8],
      [9, 13],
    ]);
    expect(matchRanges("Echo cancellation", "cance ancel")).toEqual([[5, 11]]);
    expect(matchRanges("Echo", "zz")).toEqual([]);
  });
});
