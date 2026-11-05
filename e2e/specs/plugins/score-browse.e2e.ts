// AC-9.9, AC-9.31, AC-9.32, AC-9.33, AC-9.37 and AC-9.38 (spec 09 §4.11): the security score in
// Browse, on the details page and in the install prompt, against the fixture registries of
// helpers/pluginenv.ts. No Mumble server is needed: Browse and install work without one.
//
// Expected scores (formula version 1, crates/plugins/score-weights.toml). The fixture registry
// lists every plugin with licence MIT and a plain-http source link, and the author key is new to
// the device, so F = 1 + 10 % (no https source link) − 5 % (MIT) + 10 % (new key) = 1.15:
//   test-score-low       ui.panel                                       E = 1  → 1    Low
//   test-score-medium    net.origins, 3 yellow, ui.panel                E = 39 → 45   Medium
//   test-score-high      net.origins 20, web 10+4+4, relay 8, 4 yellow 24, ui.panel 1,
//                        2 extra websites 12                            E = 83 → 95   High
//   test-score-floor     net.origins, ui.panel                          E = 21 → 24 → 30 (floor)
//   test-score-mismatch  3 yellow, ui.panel                             E = 19 → 22 listed; the
//                        package declares a proprietary licence (+5 % instead of −5 %) → 24
//
// Not automatable here (manual or covered by the engine's unit tests and test vectors):
// - AC-9.32 "changing only the source link": the fixture registry always lists a plain-http
//   source link, so only the licence change is checked (listing vs package of test-score-mismatch).
// - AC-9.33 "every trust credit": the https source link and the reproducible-build claim can't be
//   staged with `serve-registry`; the spec uses the credits it can stage (OSI licence, key known
//   400 days through MUMCORD_TEST_KEY_AGE, Expected key entered and matched).
// - AC-9.38 checks every request the fixture registries received (path, query, body). A packet
//   capture of the whole container (other hosts) stays a manual check.
import { $, $$, browser, expect } from "@wdio/globals";
import { app, pluginenv } from "../../helpers/index.ts";
import {
  addRepository,
  cancelPrompt,
  card,
  chipLevel,
  chipScore,
  chipText,
  closeSettings,
  openPlugins,
  officialLabel,
  openPrompt,
  prompt,
  promptInstall,
  waitForCard,
} from "../../helpers/score-ui.ts";
import { completeWizard } from "../../helpers/voice-ui.ts";

const LOW = "Fixture score-low";
const MEDIUM = "Fixture score-medium";
const HIGH = "Fixture score-high";
const FLOOR = "Fixture score-floor";
const MISMATCH = "Fixture score-mismatch";
const OFFICIAL = "Fixture Strip";

const EXPECTED: readonly { name: string; score: number; band: string; word: string }[] = [
  { name: LOW, score: 1, band: "low", word: "Low risk" },
  { name: MEDIUM, score: 45, band: "medium", word: "Medium risk" },
  { name: HIGH, score: 95, band: "high", word: "High risk" },
  { name: FLOOR, score: 30, band: "medium", word: "Medium risk" },
  { name: MISMATCH, score: 22, band: "low", word: "Low risk" },
];

const HARD_STOPS: readonly { name: string; reason: string }[] = [
  { name: "Fixture stop-http", reason: "it wants to use a website that is not encrypted" },
  { name: "Fixture stop-ip", reason: "it wants to use a website given as a number (IP address)" },
  { name: "Fixture stop-wildcard", reason: "it wants to use a whole group of websites (wildcard)" },
  {
    name: "Fixture stop-userinfo",
    reason: "it wants to use a website address with a user name or password",
  },
  { name: "Fixture stop-private", reason: "it wants to use a website on your own computer or network" },
];

let fixtures: pluginenv.PluginFixtures;

function thirdPartyUrl(): string {
  if (fixtures.thirdPartyUrl === undefined) throw new Error("no third-party repository");
  return fixtures.thirdPartyUrl;
}

/** Starts with a new profile: first-start wizard, Developer mode, the fixture repository. */
async function freshClient(extraEnv: Record<string, string> = {}, expectedKey?: string) {
  await app.restart({ freshProfile: true, env: pluginenv.appEnv(extraEnv) });
  await completeWizard("Alice");
  await addRepository(thirdPartyUrl(), expectedKey);
  await openPlugins("Browse");
  await waitForCard(HIGH);
}

/** Reads every expected card's chip as `name → "<word> · <score>"`. */
async function browseScores(): Promise<Record<string, string>> {
  const result: Record<string, string> = {};
  for (const { name } of EXPECTED) {
    await waitForCard(name);
    result[name] = await chipText(card(name));
  }
  return result;
}

const detailsScore = () => $("section.pd-score");

/** Opens the details page of a Browse card. */
async function openDetails(name: string): Promise<void> {
  await card(name).$(`button[aria-label='Details of ${name}']`).click();
  await detailsScore().waitForDisplayed({ timeout: 30_000 });
}

async function backToBrowse(): Promise<void> {
  await $("button.pd-back").click();
  await waitForCard(HIGH);
}

/** The text of the formula line, whitespace collapsed. */
async function formulaLine(scope: ReturnType<typeof $>): Promise<string> {
  const text = await browser.execute(
    (el) => ((el as unknown as Element).textContent as string | null) ?? "",
    (await scope.$("p.risk-formula").getElement()) as unknown as HTMLElement,
  );
  return text.replace(/\s+/g, " ").trim();
}

/** The effect column of one breakdown table ("+20", "−5 %", "no change", …). */
async function effects(scope: ReturnType<typeof $>, table: number): Promise<string[]> {
  return browser.execute(
    (el, index) => {
      const tables = (el as unknown as Element).querySelectorAll("table.risk-table");
      const table = tables.item(index) as Element | null;
      const rows = table === null ? [] : [...table.querySelectorAll("tbody tr")];
      return rows.map((row) => {
        const cell = row.querySelectorAll("td").item(1) as Element | null;
        if (cell === null) return "";
        const copy = cell.cloneNode(true) as Element;
        for (const hidden of copy.querySelectorAll(".visually-hidden")) hidden.remove();
        return (copy.textContent as string | null)?.replace(/\s+/g, " ").trim() ?? "";
      });
    },
    (await scope.getElement()) as unknown as HTMLElement,
    table,
  );
}

/** Sums "+N" / "−N" values (points or percent) of an effect column; "no change" is 0. */
function sum(values: string[]): number {
  return values.reduce((total, value) => {
    const match = /([+−-])\s*(\d+)/.exec(value);
    if (match === null) return total;
    return total + (match[1] === "+" ? 1 : -1) * Number(match[2]);
  }, 0);
}

describe("plugins: security score in Browse, details and the install prompt (AC-9.9, AC-9.31–9.33, AC-9.37, AC-9.38)", () => {
  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await pluginenv.startPluginFixtures({ official: {}, thirdParty: {} });
    await freshClient();
  });

  after(async () => {
    await pluginenv.stopPluginFixtures();
  });

  it("AC-9.31: every unofficial card shows its chip with the expected score and band", async () => {
    for (const { name, score, band, word } of EXPECTED) {
      await waitForCard(name);
      expect(await chipLevel(card(name))).toBe(band);
      expect(await chipScore(card(name))).toBe(score);
      expect(await chipText(card(name))).toBe(`${word} · ${String(score)}`);
    }
  });

  it("AC-9.31: official cards show “Official · reviewed” without a number", async () => {
    await waitForCard(OFFICIAL);
    await expect(officialLabel(card(OFFICIAL))).toHaveText("✓ Official · reviewed");
    expect(await chipScore(card(OFFICIAL))).toBeNull();
    // No official card carries a number at all.
    const officialCards = await $$("li.plugin-card").filter(async (c) =>
      officialLabel(c).isExisting(),
    );
    expect(officialCards.length).toBeGreaterThan(0);
    for (const c of officialCards) await expect(c.$(".risk-chip__score")).not.toBeExisting();
  });

  it("AC-9.32: the details page lists every factor and the numbers add up with the formula line", async () => {
    await openDetails(HIGH);
    const score = detailsScore();
    await expect(score.$(".pd-score-num")).toHaveText("95");
    // Open by default for High (and Medium).
    await expect(score.$("details.risk-details")).toHaveAttribute("open");
    const tables = score.$$("table.risk-table");
    expect(await tables.length).toBe(3);
    // One row per permission plus websites and package size; four trust factors (Expected key only
    // shows when it matched); the history part.
    const capabilities = await effects(score, 0);
    const trust = await effects(score, 1);
    const history = await effects(score, 2);
    expect(capabilities.length).toBeGreaterThanOrEqual(11);
    expect(trust.length).toBe(4);
    expect(sum(capabilities)).toBe(83);
    expect(sum(trust)).toBe(15);
    expect(sum(history)).toBe(0);
    await expect(score.$("//table[contains(@class,'risk-table')][3]")).toHaveText(
      expect.stringContaining("No history available"),
    );
    expect(await formulaLine(score)).toBe(
      "Score = 83 points × 1.15 (+15 %) + 0 = 95. Formula version 1 · How the score is calculated",
    );
    // The honesty text sits on the details page (FR-9.79).
    await expect(score).toHaveText(expect.stringContaining("This score is an estimate"));
    await backToBrowse();

    // Low risk: the breakdown is there but collapsed.
    await openDetails(LOW);
    const low = detailsScore();
    await expect(low.$("details.risk-details")).toBeExisting();
    await expect(low.$("details.risk-details")).not.toHaveAttribute("open");
    await backToBrowse();
  });

  it("AC-9.32: changing only the licence changes the score exactly as the table says", async () => {
    // Listing: MIT (−5 %); package: proprietary (+5 %). Everything else is the same.
    await openDetails(MISMATCH);
    const listed = detailsScore();
    expect(await formulaLine(listed)).toContain("Score = 19 points × 1.15 (+15 %) + 0 = 22.");
    await backToBrowse();
    await openPrompt(MISMATCH);
    await prompt().$("button=Show the full breakdown").click();
    expect(await formulaLine(prompt())).toContain("Score = 19 points × 1.25 (+25 %) + 0 = 24.");
    await cancelPrompt();
  });

  it("AC-9.37: a package scoring higher than its listing says so and shows the package score", async () => {
    expect(await chipScore(card(MISMATCH))).toBe(22);
    await openPrompt(MISMATCH);
    const summary = prompt().$(".plugin-prompt-score");
    await expect(summary.$("p.plugin-prompt-score-note")).toHaveText(
      "This plugin scores higher than its listing said",
    );
    expect(await chipScore(summary)).toBe(24);
    await expect(summary).toHaveText(expect.stringContaining("Score 24 of 100"));
    await cancelPrompt();
  });

  it("AC-9.9: each hard-stop fixture can't be installed; the prompt shows the reason and no score", async () => {
    for (const { name, reason } of HARD_STOPS) {
      await waitForCard(name);
      await openPrompt(name);
      const refused = prompt().$(".plugin-prompt-refused");
      await expect(refused).toBeDisplayed();
      await expect(refused.$("p")).toHaveText(`Cannot be installed: ${reason}.`);
      await expect(prompt().$(".plugin-prompt-score")).not.toBeExisting();
      await expect(prompt().$(".risk-chip__score")).not.toBeExisting();
      await expect(promptInstall()).not.toBeExisting();
      await cancelPrompt();
    }
  });

  it("AC-9.32: the same packages score the same after a restart and on a second client", async () => {
    const before = await browseScores();
    await closeSettings();
    await app.restart({ env: pluginenv.appEnv() });
    await openPlugins("Browse");
    expect(await browseScores()).toEqual(before);
    // A second client: a new profile on the same machine, which also first sees the key today.
    await freshClient();
    expect(await browseScores()).toEqual(before);
  });

  it("AC-9.38: the registries only received index, signature, media and package requests", () => {
    // Everything this spec did so far (browsing, details, five prompts, two restarts) went through
    // these servers. No request may carry a query, a body, a score or an installed-plugin id.
    const allowed =
      /^(GET|HEAD) \/(?:v1(?:\.1)?\/(?:index|revoked|permissions)\.json(?:\.minisig)?|v1\/media\/[a-z0-9-]+\/icon\.png|packages\/[a-z0-9-]+-\d+\.\d+\.\d+\.mcplugin(?:\.minisig)?) \d{3}$/;
    const seen = [...pluginenv.requests("official"), ...pluginenv.requests("third-party")].filter(
      (line) => !line.startsWith("POST /_ctl/"),
    );
    expect(seen.length).toBeGreaterThan(0);
    expect(seen.filter((line) => !allowed.test(line))).toEqual([]);
    // Packages were downloaded only for plugins whose prompt was opened.
    const packages = seen.filter((line) => line.includes(" /packages/"));
    expect(packages.length).toBeGreaterThan(0);
  });

  it("AC-9.33: a plugin with net.origins and every trust credit the fixture can stage scores 30", async () => {
    if (fixtures.thirdPartyKey === undefined) throw new Error("no third-party key");
    // Key known for 400 days (−10 %), Expected key entered and matched (−5 %), MIT (−5 %); the
    // fixture source link is plain http (+10 %): F = 0.90, 21 × 0.90 = 19 → raised to 30.
    await freshClient(
      { MUMCORD_TEST_KEY_AGE: `${fixtures.thirdPartyKey}=400` },
      fixtures.thirdPartyKey,
    );
    expect(await chipText(card(FLOOR))).toBe("Medium risk · 30");
    await openDetails(FLOOR);
    const score = detailsScore();
    expect(await formulaLine(score)).toBe(
      "Score = 21 points × 0.90 (−10 %) + 0 = 19. Raised to 30 because the plugin can reach the internet. Formula version 1 · How the score is calculated",
    );
    await backToBrowse();
  });
});
