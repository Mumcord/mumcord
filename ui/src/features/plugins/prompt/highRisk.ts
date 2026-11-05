import type { TFunction } from "i18next";
import type { PluginScoreFactor } from "../../../api";

/** What is known about a plugin to explain why it is high risk. */
export interface HighRiskFacts {
  /** It asks for `ui.webview`. */
  webPages: boolean;
  /** Number of websites it declares. */
  websites: number;
  /** This computer has never seen the author's key before (score factor "key_age.new"). */
  newAuthor: boolean;
}

/** True when the score says the author's key is new to this computer (FR-9.74). */
export function hasNewAuthorKey(factors: readonly PluginScoreFactor[]): boolean {
  return factors.some((f) => f.kind === "keyAge" && f.detailKey === "key_age.new");
}

/**
 * The text of the high-risk box in the mockup's wording, built only from clauses that are true
 * for this plugin. Empty when none applies: the box then shows its title and tick only.
 */
export function highRiskSentences(t: TFunction<"pluginGate">, facts: HighRiskFacts): string[] {
  const sentences: string[] = [];
  if (facts.webPages && facts.websites > 0) {
    sentences.push(t("highRiskParts.webPagesAndWebsites", { count: facts.websites }));
  } else if (facts.webPages) {
    sentences.push(t("highRiskParts.webPages"));
  } else if (facts.websites > 0) {
    sentences.push(t("highRiskParts.websites", { count: facts.websites }));
  }
  if (facts.websites > 0) sentences.push(t("highRiskParts.cantSee"));
  if (facts.newAuthor) sentences.push(t("highRiskParts.newAuthor"));
  if (sentences.length > 0) sentences.push(t("highRiskParts.trust"));
  return sentences;
}
