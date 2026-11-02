// AC-6.4 and AC-6.7 (spec 06): Dice roller between two Mumcord clients A and B, with a plain
// Mumble user C (mumbot). A's roll is verified by B within 5 s and both see the same result; C
// only sees the fallback line; rolling alone is "Unverified (rolled alone)". Typing "/roll"
// offers the Dice command, and the typed command itself never reaches the chat.
import { $, browser, expect } from "@wdio/globals";
import { app, apps, registry, testenv } from "../../helpers/index.ts";
import { composer, joinChannel, messageTexts, ScriptBot } from "../../helpers/chat-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { addAndConnect, completeWizard, personRow } from "../../helpers/voice-ui.ts";

const VERIFIED_WITHIN_MS = 5_000;
/**
 * Plugin messages travel only on the voice server (FR-6.33, H3 addendum), so A and B join the
 * channel instead of just reading it.
 */
const ROOT = "Mumcord Test Server";

async function openPlugins(tab: "Browse" | "Installed"): Promise<void> {
  await openSettingsGroup("Plugins");
  await $(
    `//button[@role='tab'][contains(@class,'plugins-tab')][starts-with(normalize-space(.),'${tab}')]`,
  ).click();
}

/**
 * The fixture registry serves plain http on 127.0.0.1, which the client only accepts in
 * Developer mode (FR-6.74).
 */
async function turnOnDeveloperMode(): Promise<void> {
  await openSettingsGroup("Advanced & Developer");
  const developerMode = $("//input[@role='switch'][@id=//label[.='Developer mode']/@for]");
  await developerMode.waitForExist({ timeout: 10_000 });
  await developerMode.click();
  await expect(developerMode).toBeSelected();
}

async function closeSettings(): Promise<void> {
  await browser.keys(["Escape"]);
  await $(".overlay__dialog").waitForExist({ reverse: true, timeout: 10_000 });
}

async function installDice(): Promise<void> {
  await turnOnDeveloperMode();
  await openPlugins("Browse");
  const install = $("button[aria-label='Install Dice roller']");
  await install.waitForClickable({ timeout: 30_000 });
  await install.click();
  // The prompt checks the package first (progress bar), then offers Install.
  const confirm = $(
    "//div[contains(concat(' ',@class,' '),' plugin-prompt ')]//button[normalize-space(.)='Install']",
  );
  await confirm.waitForClickable({ timeout: 60_000 });
  await confirm.click();
  await expect($(".plugin-toast")).toHaveText(expect.stringContaining("Dice roller installed"), {
    wait: 60_000,
  });
  await closeSettings();
}

/**
 * Text of the newest Dice card in the open chat, or "" when there is none. The dice are drawn
 * as faces, so their values come from the images' alt text, appended after the visible text.
 */
async function lastDiceCard(): Promise<string> {
  return browser.execute(() => {
    const cards = document.querySelectorAll<HTMLElement>(
      "section.plugin-chat-card[aria-label='Card from Dice roller']",
    );
    if (cards.length === 0) return "";
    const card = cards[cards.length - 1];
    const faces = Array.from(card.querySelectorAll<HTMLImageElement>("img[alt]"), (i) => i.alt);
    return `${card.innerText}\n${faces.join(" ")}`;
  });
}

async function waitCard(status: string, timeout: number): Promise<string> {
  let text = "";
  await browser.waitUntil(
    async () => {
      text = await lastDiceCard();
      return text.includes(status);
    },
    { timeout, interval: 100, timeoutMsg: `no Dice card with "${status}" (last: ${text})` },
  );
  return text;
}

/** The dice values of a card or fallback line, e.g. "11 (4, 4)" → total 11, dice [4, 4]. */
function rolled(line: string): { total: string; dice: string } {
  const match = /= (-?\d+) \(([^)]*)\)/.exec(line);
  if (!match) throw new Error(`no roll result in ${JSON.stringify(line)}`);
  return { total: match[1], dice: match[2] };
}

/**
 * Replaces the composer text the way React sees typing (native setter + input event): WebDriver
 * `setValue` keeps the old text of this controlled box, and fast key-by-key typing drops keys.
 */
async function typeInComposer(text: string): Promise<void> {
  const box = composer();
  await box.waitForEnabled();
  await box.click();
  await browser.execute((value: string) => {
    const field = document.querySelector<HTMLTextAreaElement>("textarea[aria-label^='Message']");
    if (!field) throw new Error("no composer");
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set?.call(
      field,
      value,
    );
    field.dispatchEvent(new Event("input", { bubbles: true }));
  }, text);
  await expect(box).toHaveValue(text);
}

describe("plugins: Dice roller between two clients", () => {
  let fixtures: registry.FixtureRegistry;
  let carol: ScriptBot | undefined;
  const plainUser = (): ScriptBot => {
    if (!carol) throw new Error("C (mumbot) is not connected");
    return carol;
  };

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await registry.start();
    await testenv.up("v15");
  });

  after(async () => {
    carol?.stop();
    await apps.stopB();
    await fixtures.stop();
  });

  it("A and B install Dice and share a channel with C", async () => {
    await app.restart({ env: fixtures.env() });
    await completeWizard("Alice");
    await addAndConnect();
    await joinChannel(ROOT);
    await installDice();
    await apps.startB({ env: fixtures.env() });
    await apps.onB(async () => {
      await completeWizard("Bob");
      await addAndConnect();
      await joinChannel(ROOT);
      await installDice();
      await personRow("Alice").waitForDisplayed({ timeout: 20_000 });
    });
    carol = new ScriptBot({ name: "Carol", timeout: 30 });
    await carol.connected();
    await personRow("Bob").waitForDisplayed({ timeout: 20_000 });
    await personRow("Carol").waitForDisplayed({ timeout: 20_000 });
  });

  it("AC-6.7: typing /roll offers the Dice roller command", async () => {
    await typeInComposer("/roll");
    const option = $("//div[@role='listbox'][@aria-label='Plugin commands']//*[@role='option']");
    await option.waitForDisplayed({ timeout: 10_000 });
    await expect(option).toHaveText(expect.stringContaining("/roll"));
    await expect(option).toHaveText(expect.stringContaining("Dice roller"));
  });

  it("AC-6.4: A's roll is verified on A and B within 5 s, C gets the fallback line", async () => {
    const fallback = plainUser().run('expect message --from Alice --contains "rolled 2d6+3"', 30_000);
    const box = composer();
    await typeInComposer("/roll 2d6+3");
    await browser.keys("Enter");
    const sent = Date.now();
    const cardA = await waitCard("Verified", VERIFIED_WITHIN_MS);
    const cardB = await apps.onB(() =>
      waitCard("Verified", Math.max(VERIFIED_WITHIN_MS - (Date.now() - sent), 1)),
    );
    expect(cardA).not.toContain("Unverified");
    expect(cardB).not.toContain("Unverified");

    const line = await fallback;
    expect(line).toContain("[Dice roller]");
    expect(line).not.toContain("unverified");
    const result = rolled(line);
    for (const card of [cardA, cardB]) {
      expect(card).toContain(result.total);
      for (const die of result.dice.split(", ")) expect(card).toContain(die);
    }

    // AC-6.7: the typed command went to the plugin, never into the chat (A, B and C).
    await expect(box).toHaveValue("");
    expect(line).not.toContain("/roll");
    for (const texts of [await messageTexts(), await apps.onB(messageTexts)]) {
      expect(texts.filter((t) => t.includes("/roll"))).toEqual([]);
    }
  });

  it("AC-6.4: rolling while B has Dice turned off is unverified", async () => {
    await apps.onB(async () => {
      await openPlugins("Installed");
      const toggle = $("input[role='switch'][aria-label='Dice roller on']");
      await toggle.waitForExist({ timeout: 10_000 });
      await toggle.click();
      await browser.waitUntil(async () => !(await toggle.isSelected()), {
        timeout: 10_000,
        timeoutMsg: "Dice did not turn off on B",
      });
      await closeSettings();
    });
    const fallback = plainUser().run(
      'expect message --from Alice --contains "unverified, rolled alone"',
      30_000,
    );
    await typeInComposer("/roll d20");
    await browser.keys("Enter");
    // The fallback line may follow the card up to 8 s later (FR-6.44 addendum).
    await waitCard("Unverified (rolled alone)", 15_000);
    await fallback;
  });
});
