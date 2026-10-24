import { describe, expect, it } from "vitest";
import { i18n, applyLanguage } from ".";
import { detectOsLanguage, resolveLanguage } from "./language";
import { resources, SUPPORTED_LANGUAGES, type TranslationTree } from "./resources";

function keys(tree: TranslationTree, prefix = ""): string[] {
  return Object.entries(tree).flatMap(([key, value]) =>
    typeof value === "string" ? [`${prefix}${key}`] : keys(value, `${prefix}${key}.`),
  );
}

describe("translation files", () => {
  it("has the same namespaces in every language", () => {
    const expected = Object.keys(resources.en).sort();
    expect(expected).toEqual(expect.arrayContaining(["app", "common"]));
    for (const language of SUPPORTED_LANGUAGES) {
      expect(Object.keys(resources[language]).sort()).toEqual(expected);
    }
  });

  it.each(Object.keys(resources.en))("has the same keys in every language for %s", (ns) => {
    const expected = keys(resources.en[ns] ?? {}).sort();
    for (const language of SUPPORTED_LANGUAGES) {
      expect(keys(resources[language][ns] ?? {}).sort()).toEqual(expected);
    }
  });

  it("has no empty translations", () => {
    for (const language of SUPPORTED_LANGUAGES) {
      for (const tree of Object.values(resources[language])) {
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
    }
  });
});

describe("language selection", () => {
  it("uses the first supported OS language", () => {
    expect(detectOsLanguage(["fr-FR", "de-AT", "en-US"])).toBe("de");
    expect(detectOsLanguage(["es"])).toBe("es");
  });

  it("falls back to English for unsupported OS languages", () => {
    expect(detectOsLanguage(["fr-FR", "ja"])).toBe("en");
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
});
