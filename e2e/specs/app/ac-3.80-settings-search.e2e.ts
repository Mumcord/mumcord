import { $, $$, browser, expect } from "@wdio/globals";
import { runFirstStartWizard } from "../../helpers/wizard-util.ts";
import { openAllSettings, searchSettings, settingsTab } from "../../helpers/settings-ui.ts";

// AC-3.80, search part (#161): "relay" lists grouped results by top-level group with a count,
// including a setting whose label does not contain the word and is only found through its
// synonyms ("relay", "hide my IP" → peer-to-peer); picking it opens its group and highlights the row.

/** Found only through the peer-to-peer synonyms: the label itself never says "relay". */
const SYNONYM_HIT = "Allow direct connections to other Mumcord users";

function resultGroup(group: string) {
  return $(
    `//*[contains(@class,'settings-search__results')]//section[.//h3[contains(@class,'settings-search__group')][starts-with(normalize-space(.),'${group}')]]`,
  );
}

describe("AC-3.80 settings search", () => {
  before(async () => {
    await runFirstStartWizard("SearchTester");
    await openAllSettings();
  });

  it("groups the results for \"relay\" by top-level group, synonyms included", async () => {
    await searchSettings("relay");
    // The results replace the group list.
    await expect($(".settings__nav")).not.toBeDisplayed();
    const privacy = resultGroup("Privacy & Connections");
    await privacy.waitForDisplayed();
    const results = await privacy.$$("button.settings-search__result").map((b) => b.getText());
    expect(results.length).toBeGreaterThanOrEqual(2);
    const count = await privacy.$(".settings-search__count").getText();
    expect(count).toBe(`${String(results.length)} results`);
    expect(results.some((text) => text.startsWith(SYNONYM_HIT))).toBe(true);
    // Matches in visible text are marked.
    expect(await $$(".settings-search__results mark.settings-search__match").length).toBeGreaterThan(0);
  });

  it("navigates with the arrow keys and clears with Escape", async () => {
    await $("input.settings-search__input").click();
    await browser.keys(["ArrowDown"]);
    const focused = await browser.execute(() => document.activeElement?.className ?? "");
    expect(focused).toContain("settings-search__result");
    await browser.keys(["Escape"]);
    await expect($("input.settings-search__input")).toHaveValue("");
    await expect($(".settings__nav")).toBeDisplayed();
  });

  it("picking the synonym hit opens its group and highlights the row", async () => {
    await searchSettings("relay");
    const hit = resultGroup("Privacy & Connections").$(
      `.//button[contains(@class,'settings-search__result')][starts-with(normalize-space(.),'${SYNONYM_HIT}')]`,
    );
    await hit.click();
    await expect(settingsTab("Privacy & Connections")).toHaveAttribute("aria-selected", "true");
    const highlighted = $(".settings-highlight");
    await highlighted.waitForExist({ timeout: 5_000 });
    expect(await highlighted.getText()).toContain(SYNONYM_HIT);
    await expect(highlighted).toBeDisplayedInViewport();
  });
});
