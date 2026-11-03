import { beforeAll, describe, expect, it } from "vitest";
import { applyLanguage, i18n } from "../../../i18n";
import { hasNewAuthorKey, highRiskSentences } from "./highRisk";

beforeAll(async () => {
  await applyLanguage("en");
});

describe("highRiskSentences", () => {
  const t = () => i18n.getFixedT("en", "pluginGate");

  it("leaves out every clause that does not apply", () => {
    expect(highRiskSentences(t(), { webPages: false, websites: 0, newAuthor: false })).toEqual([]);
    expect(highRiskSentences(t(), { webPages: true, websites: 0, newAuthor: false })).toEqual([
      "It can open its own web pages.",
      "Only install it if you know and trust its author.",
    ]);
    expect(highRiskSentences(t(), { webPages: false, websites: 1, newAuthor: true })).toEqual([
      "It can talk to 1 website you can't check.",
      "Mumcord cannot see what those websites send to the plugin or what the plugin sends to them.",
      "It was published by someone who is new to this computer.",
      "Only install it if you know and trust its author.",
    ]);
  });
});

describe("hasNewAuthorKey", () => {
  it("is true only for a key this computer has never seen", () => {
    const factor = (detailKey: string) => ({
      kind: "keyAge" as const,
      effect: { kind: "percent" as const, value: 0 },
      detailKey,
    });
    expect(hasNewAuthorKey([factor("key_age.new")])).toBe(true);
    expect(hasNewAuthorKey([factor("key_age.known")])).toBe(false);
  });
});
