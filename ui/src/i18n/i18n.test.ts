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
