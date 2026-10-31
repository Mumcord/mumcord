import { join } from "node:path";
import { browser, expect, $, $$ } from "@wdio/globals";
import { app, artifactsDir, repoRoot, testenv } from "../../helpers/index.ts";
import { hasClass } from "../../helpers/selectors.ts";
import {
  addFirstServer,
  closeMessagesOverview,
  finishWizard,
  joinChannel,
  openMessagesOverview,
  OutputWav,
  pmRow,
  ScriptBot,
  selectChannel,
  silentWav,
  sleep,
  USER,
  waitMessage,
} from "../../helpers/chat-ui.ts";

/** Certificate of the bot: the same identity across both of its names. */
const CERT = "BlockQA";
const speech = join(repoRoot, "testenv", "audio", "speech.wav");

function treePerson(name: string) {
  return $(`//div[${hasClass("tree-person")} and contains(.,'${name}')]`);
}

function collapsedMessages() {
  return $$("[data-testid='blocked-message']");
}

async function blockedMark(name: string): Promise<boolean> {
  return treePerson(name).$(".blocked-badge").isExisting();
}

async function hasConversation(name: string): Promise<boolean> {
  await openMessagesOverview();
  const exists = await pmRow(name).isExisting();
  await closeMessagesOverview();
  return exists;
}

/** Peak level while the bot plays speech in the user's channel. */
async function voicePeak(out: OutputWav, bot: ScriptBot): Promise<number> {
  const before = out.length();
  await bot.run(`play ${speech}`, 20_000);
  await sleep(1000);
  const after = out.length();
  expect(after - before).toBeGreaterThan(48_000 * 6);
  return out.peakDb(before, after);
}

async function openPrivacySettings(): Promise<void> {
  const button = $("button[aria-label='Quick settings']");
  await button.waitForClickable();
  await button.click();
  await $("//button[@role='menuitem'][contains(., 'All settings')]").click();
  await $("//button[@role='tab' and .='Privacy']").click();
}

// AC-3.71 (issue #140): block the second client → their private messages neither show nor
// notify, their channel messages collapse, their voice is silent for the user; the block follows
// the certificate across a name change; Unblock in Settings → Privacy restores everything.
describe("chat: block users (AC-3.71)", () => {
  const out = new OutputWav(join(artifactsDir, "chat-block-out.wav"));
  let blockBot: ScriptBot;

  before(async () => {
    await testenv.up("v15");
    await app.restart({ audio: `file:${silentWav("block-silence.wav", 600)}:${out.path}` });
    await finishWizard();
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
    await selectChannel("Lobby");
    blockBot = new ScriptBot({ name: "BlockBot", cert: CERT });
    await blockBot.connected();
    await blockBot.run("join Lobby");
    // Warm-up: the output recording restarts when the first remote voice arrives.
    await blockBot.run(`play ${speech}`, 20_000);
    await sleep(1500);
  });

  after(() => {
    blockBot.stop();
  });

  it("blocks from the person menu after the confirmation", async () => {
    await treePerson("BlockBot").waitForDisplayed({ timeout: 10_000 });
    await treePerson("BlockBot").click({ button: "right" });
    const item = $("//*[@role='menuitem'][normalize-space()='Block…']");
    await item.waitForClickable();
    await item.click();
    const dialog = $("//dialog[.//h2[normalize-space()='Block BlockBot?']]");
    await dialog.waitForDisplayed();
    await expect(dialog).toHaveText(expect.stringContaining("can still see you"));
    await $("[data-testid='block-confirm']").click();
    await browser.waitUntil(() => blockedMark("BlockBot"), {
      timeoutMsg: "BlockBot is not marked as blocked in the channel tree",
    });
  });

  it("collapses their channel messages; Show reveals one", async () => {
    await blockBot.run('say --channel Lobby "blocked hello"');
    await $("[data-testid='blocked-message']").waitForDisplayed({ timeout: 15_000 });
    await expect($("//*[contains(@class,'chat-message')]//*[contains(.,'blocked hello')]")).not.toBeExisting();
    await $("//button[normalize-space()='Show']").click();
    await waitMessage("blocked hello");
  });

  it("shows and notifies nothing for their private messages", async () => {
    await blockBot.run(`pm ${USER} "secret ping"`);
    await sleep(2000);
    expect(await hasConversation("BlockBot")).toBe(false);
    await expect($("[data-testid='notification-bell']")).toHaveAttribute("aria-label", "Notifications");
  });

  it("their voice is silent for the user", async () => {
    const peak = await voicePeak(out, blockBot);
    console.warn(`AC-3.71 peak while BlockBot talks blocked: ${peak.toFixed(1)} dBFS`);
    expect(peak).toBeLessThan(-60);
  });

  it("stays blocked after a name change on the same certificate", async () => {
    blockBot.stop();
    blockBot = new ScriptBot({ name: "BlockBot2", cert: CERT });
    await blockBot.connected();
    await blockBot.run("join Lobby");
    await treePerson("BlockBot2").waitForDisplayed({ timeout: 10_000 });
    await browser.waitUntil(() => blockedMark("BlockBot2"), {
      timeoutMsg: "BlockBot2 is not marked as blocked",
    });
    const before = (await collapsedMessages()).length;
    await blockBot.run('say --channel Lobby "renamed hello"');
    await browser.waitUntil(async () => (await collapsedMessages()).length > before, {
      timeout: 15_000,
      timeoutMsg: "the renamed bot's message is not collapsed",
    });
  });

  it("Unblock in Settings → Privacy restores messages and voice", async () => {
    await openPrivacySettings();
    const unblock = $("//button[@aria-label='Unblock BlockBot']");
    await unblock.waitForClickable();
    await unblock.click();
    await $("[data-testid='blocked-empty']").waitForDisplayed();
    await browser.keys("Escape");
    await $(".overlay__dialog").waitForDisplayed({ reverse: true });

    await browser.waitUntil(async () => !(await blockedMark("BlockBot2")), {
      timeoutMsg: "BlockBot2 is still marked as blocked",
    });
    await expect(collapsedMessages()).toBeElementsArrayOfSize(0);
    await waitMessage("renamed hello");
    await blockBot.run(`pm ${USER} "hello again"`);
    await browser.waitUntil(() => hasConversation("BlockBot2"), {
      timeout: 15_000,
      timeoutMsg: "the private message after unblocking did not arrive",
    });
    const peak = await voicePeak(out, blockBot);
    console.warn(`AC-3.71 control peak after unblock: ${peak.toFixed(1)} dBFS`);
    expect(peak).toBeGreaterThan(-45);
  });
});
