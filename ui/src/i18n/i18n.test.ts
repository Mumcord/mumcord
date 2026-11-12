import { beforeAll, describe, expect, it } from "vitest";
import type { Language } from "../api";
import { i18n, applyLanguage } from ".";
import { detectOsLanguage, resolveLanguage } from "./language";
import {
  ENDONYMS,
  english,
  loadBundles,
  SUPPORTED_LANGUAGES,
  type Bundles,
  type TranslationTree,
} from "./resources";

function keys(tree: TranslationTree, prefix = ""): string[] {
  return Object.entries(tree).flatMap(([key, value]) =>
    typeof value === "string" ? [`${prefix}${key}`] : keys(value, `${prefix}${key}.`),
  );
}

/** The text at a dotted key, or `undefined` where a namespace or the key is absent. */
function textOf(tree: TranslationTree | undefined, key: string): string | undefined {
  let node: string | TranslationTree | undefined = tree;
  for (const part of key.split(".")) {
    node = typeof node === "object" ? node[part] : undefined;
  }
  return typeof node === "string" ? node : undefined;
}

/** The texts of a key: its own, plus every CLDR plural form it has (`key_one`, `key_other`, …). */
function textsOf(tree: TranslationTree | undefined, key: string): string[] {
  const forms = ["zero", "one", "two", "few", "many", "other"];
  return [key, ...forms.map((form) => `${key}_${form}`)]
    .map((each) => textOf(tree, each))
    .filter((text): text is string => text !== undefined);
}

const PLURAL = /_(zero|one|two|few|many|other)$/;
/** The keys a language must have: English's keys, with each plural set in every CLDR form of that language. */
function expectedKeys(language: Language, englishKeys: string[]): string[] {
  const forms = new Intl.PluralRules(language).resolvedOptions().pluralCategories;
  const plurals = new Set(
    englishKeys.filter((key) => PLURAL.test(key)).map((key) => key.replace(PLURAL, "")),
  );
  return [
    ...englishKeys.filter((key) => !PLURAL.test(key)),
    ...[...plurals].flatMap((base) => forms.map((form) => `${base}_${form}`)),
  ].sort();
}

const bundles = {} as Record<Language, Bundles>;
beforeAll(async () => {
  for (const language of SUPPORTED_LANGUAGES) bundles[language] = await loadBundles(language);
});

describe("translation files", () => {
  it("lists every language once, each with its own name", () => {
    expect([...SUPPORTED_LANGUAGES].sort()).toEqual(Object.keys(ENDONYMS).sort());
    expect(SUPPORTED_LANGUAGES).toHaveLength(24);
  });

  it.each(SUPPORTED_LANGUAGES)("%s has the same namespaces as English", (language) => {
    expect(Object.keys(bundles[language]).sort()).toEqual(Object.keys(english).sort());
  });

  it.each(SUPPORTED_LANGUAGES)("%s has every English key, in its own plural forms", (language) => {
    for (const [namespace, tree] of Object.entries(english)) {
      const englishKeys = keys(tree);
      const expected = expectedKeys(language, englishKeys);
      const actual = keys(bundles[language][namespace] ?? {}).sort();
      expect({ namespace, keys: actual }).toEqual({ namespace, keys: expected });
    }
  });

  it.each(SUPPORTED_LANGUAGES)("%s has no empty translations", (language) => {
    for (const tree of Object.values(bundles[language])) {
      const empty = keys(tree).filter((key) => {
        const value = key
          .split(".")
          .reduce<TranslationTree | string | undefined>(
            (node, part) => (typeof node === "object" ? node[part] : undefined),
            tree,
          );
        return typeof value !== "string" || value.trim() === "";
      });
      expect(empty).toEqual([]);
    }
  });

  // The server info dialog shipped with English text copied into every language (#207).
  // Keys that are legitimately the same in some languages (units, symbols, loanwords) are exempt.
  const SAME_IN_SOME_LANGUAGES = new Set([
    "none",
    "sha256",
    "ping",
    "versionOnly",
    "name",
    "no",
    "tlsValid",
    "version",
    "release",
    "codecs",
    "transport",
    "tlsStatus",
    "serverSection",
    "usersOf",
    "mumcordUsersOf",
    "validRange",
    "permissionBan",
    "permissionMove",
    "permissionKick",
    "permissionListen",
  ]);
  it.each(SUPPORTED_LANGUAGES.filter((language) => language !== "en"))(
    "%s translates the server info dialog",
    (language) => {
      const info = (bundles.en.servers as { serverInfo: Record<string, string> }).serverInfo;
      const own = (bundles[language].servers as { serverInfo: Record<string, string> }).serverInfo;
      const untranslated = Object.keys(info).filter(
        (key) => own[key] === info[key] && !SAME_IN_SOME_LANGUAGES.has(key),
      );
      expect(untranslated).toEqual([]);
    },
  );

  // The 0.9.0 fallback texts (FR-7.99, FR-8.5, FR-8.52, FR-8.62, FR-8.94): a viewer that cannot be
  // served, an old relay, the path labels of a share or camera. A copy of the English text here is
  // a missed translation, not a brand name.
  const FALLBACK_TEXTS: Record<string, readonly string[]> = {
    camera: [
      "cantWatch",
      "room.relay",
      "room.direct",
      "room.both",
      "room.relayTitle",
      "room.directTitle",
      "room.bothTitle",
      "viewers.relay",
      "viewers.direct",
      "tile.full",
      "tile.failed",
      "tile.retry",
      "unavailable.noRelay",
      "unavailable.relayTooOld",
      "unavailable.relayDisallows",
      "unavailable.needsRelayMode",
      "unavailable.needsDirect",
      "errors.relayDown",
      "errors.endedByRelay",
      "errors.relayFull",
      "errors.notSameChannel",
      "errors.noHello",
    ],
    screenshare: [
      "full",
      "fullShort",
      "notices.noFanoutTitle",
      "notices.noFanoutBody",
      "notices.fanoutUnavailable",
      "notices.reasonNoRelay",
      "notices.reasonOldRelay",
      "notices.reasonDisallowed",
      "notices.reasonRefused",
      "notices.reasonLost",
      "start.pathRelay",
      "start.pathDirect",
      "liveBar.pathRelay",
      "liveBar.pathDirect",
      "viewerList.pathRelay",
      "viewerList.pathDirect",
      "viewerList.pathBroadcast",
      "viewerList.relaySummary",
      "viewerList.directSummary",
    ],
    screenviewer: [
      "needsUpdate",
      "full",
      "tooOld",
      "unreachable",
      "consentOff",
      "failed",
      "path",
      "pathRelay",
      "pathDirect",
      "header.relay",
      "header.direct",
      "header.codecViaRelay",
      "header.codecDirect",
    ],
  };

  // "Relay" and "Direct" are the usual words for the path in these languages (cs, de, fr, it, mt,
  // nl, ro, sk) and stay English there; everything else must differ.
  const FALLBACK_SAME_AS_ENGLISH = new Set([
    "cs camera.viewers.relay",
    "de camera.viewers.relay",
    "it camera.viewers.relay",
    "mt camera.viewers.relay",
    "nl camera.viewers.relay",
    "sk camera.viewers.relay",
    "fr camera.viewers.direct",
    "nl camera.viewers.direct",
    "ro camera.viewers.direct",
    "cs screenshare.liveBar.pathRelay",
    "de screenshare.liveBar.pathRelay",
    "it screenshare.liveBar.pathRelay",
    "mt screenshare.liveBar.pathRelay",
    "nl screenshare.liveBar.pathRelay",
    "sk screenshare.liveBar.pathRelay",
    "fr screenshare.liveBar.pathDirect",
    "nl screenshare.liveBar.pathDirect",
    "ro screenshare.liveBar.pathDirect",
    "cs screenshare.viewerList.pathRelay",
    "de screenshare.viewerList.pathRelay",
    "it screenshare.viewerList.pathRelay",
    "mt screenshare.viewerList.pathRelay",
    "nl screenshare.viewerList.pathRelay",
    "sk screenshare.viewerList.pathRelay",
    "fr screenshare.viewerList.pathDirect",
    "nl screenshare.viewerList.pathDirect",
    "ro screenshare.viewerList.pathDirect",
    "cs screenviewer.pathRelay",
    "de screenviewer.pathRelay",
    "it screenviewer.pathRelay",
    "mt screenviewer.pathRelay",
    "nl screenviewer.pathRelay",
    "sk screenviewer.pathRelay",
    "fr screenviewer.pathDirect",
    "nl screenviewer.pathDirect",
    "cs screenviewer.header.relay",
    "de screenviewer.header.relay",
    "it screenviewer.header.relay",
    "mt screenviewer.header.relay",
    "nl screenviewer.header.relay",
    "sk screenviewer.header.relay",
    "fr screenviewer.header.direct",
    "nl screenviewer.header.direct",
    "ro screenviewer.header.direct",
    "nl screenviewer.header.codecViaRelay",
    "fr screenviewer.header.codecDirect",
    "nl screenviewer.header.codecDirect",
    "ro screenviewer.header.codecDirect",
  ]);

  it("names only fallback texts English has", () => {
    const missing = Object.entries(FALLBACK_TEXTS).flatMap(([namespace, list]) =>
      list
        .filter((key) => textsOf(english[namespace], key).length === 0)
        .map((key) => `${namespace}.${key}`),
    );
    expect(missing).toEqual([]);
  });

  it.each(SUPPORTED_LANGUAGES.filter((language) => language !== "en"))(
    "%s translates the fallback texts",
    (language) => {
      const untranslated = Object.entries(FALLBACK_TEXTS).flatMap(([namespace, list]) =>
        list
          .filter((key) => {
            const own = textsOf(bundles[language][namespace], key).join("\u0000");
            return (
              own === textsOf(english[namespace], key).join("\u0000") &&
              !FALLBACK_SAME_AS_ENGLISH.has(`${language} ${namespace}.${key}`)
            );
          })
          .map((key) => `${namespace}.${key}`),
      );
      expect(untranslated).toEqual([]);
    },
  );
});

describe("language selection", () => {
  it("uses the first supported OS language", () => {
    expect(detectOsLanguage(["nb-NO", "de-AT", "en-US"])).toBe("de");
    expect(detectOsLanguage(["pt-BR"])).toBe("pt");
    expect(detectOsLanguage(["GA-ie"])).toBe("ga");
    expect(detectOsLanguage(["es"])).toBe("es");
  });

  it("falls back to English for unsupported OS languages", () => {
    expect(detectOsLanguage(["nb-NO", "ja"])).toBe("en");
    expect(detectOsLanguage([])).toBe("en");
  });

  it("prefers the language setting over the OS", () => {
    expect(resolveLanguage("es", ["de-DE"])).toBe("es");
    expect(resolveLanguage(null, ["de-DE"])).toBe("de");
  });

  it("switches language at runtime and updates <html lang>", async () => {
    await applyLanguage("de");
    expect(i18n.t("app:regions.channels")).toBe("Kanäle");
    expect(document.documentElement.lang).toBe("de");
    await applyLanguage("en");
    expect(i18n.t("app:regions.channels")).toBe("Channels");
  });

  it("uses the active language's plural rules", async () => {
    i18n.addResourceBundle("pl", "test", {
      files_one: "plik",
      files_few: "pliki",
      files_many: "plików",
    });
    await applyLanguage("pl");
    expect([1, 3, 5].map((count) => i18n.t("test:files", { count }))).toEqual([
      "plik",
      "pliki",
      "plików",
    ]);
    await applyLanguage("en");
  });

  it("applies only the latest choice when switching quickly", async () => {
    await Promise.all([applyLanguage("es"), applyLanguage("de")]);
    expect(i18n.language).toBe("de");
    await applyLanguage("en");
  });
});
