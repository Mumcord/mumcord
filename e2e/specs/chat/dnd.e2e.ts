import { browser, expect, $ } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  finishWizard,
  joinChannel,
  ScriptBot,
  selectChannel,
  sleep,
  unreadBadge,
  USER,
} from "../../helpers/chat-ui.ts";
import { userPanelLayout } from "../../helpers/voice-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";

function anyToast() {
  return $("//section[@aria-label='Notifications']//button[contains(@class,'toast__open')]");
}

function followButton() {
  return $("//button[contains(@class,'follow-toggle') and @aria-pressed]");
}

function dndRow() {
  return $("//button[@role='menuitemcheckbox'][starts-with(normalize-space(.), 'Do Not Disturb')]");
}

// A compact "DND" chip in the user panel's status strip (#246); the source is its accessible name.
function indicator() {
  return $(".dnd-indicator");
}

async function openQuickSettings(): Promise<void> {
  const button = $("button[aria-label='Quick settings']");
  await button.waitForClickable();
  await button.click();
  await dndRow().waitForDisplayed();
}

// AC-3.76 (FR-3.111, FR-3.112): with Do Not Disturb on, a mention plays no sound and shows no
// toast, but the unread badge counts it; the user panel says why; the switch survives a
// restart. The container has no desktop Do Not Disturb, so the settings say it can't be read.
describe("chat: Do Not Disturb (AC-3.76)", () => {
  let raidBot: ScriptBot;

  before(async () => {
    await testenv.up("v15");
    await app.restart({});
    await finishWizard();
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
    raidBot = new ScriptBot({ name: "RaidBot" });
    await raidBot.connected();
    await raidBot.run("join Raid");
    await selectChannel("Raid");
    await followButton().click();
    await expect(followButton()).toHaveAttribute("aria-pressed", "true");
    await bot(["expect", "user", USER, "--in", "Lobby", "--listening", "Raid"], { name: "EarBot" });
  });

  after(() => {
    raidBot.stop();
  });

  it("turns on from quick settings and shows the manual indicator", async () => {
    const before = await userPanelLayout();
    await openQuickSettings();
    await expect(dndRow()).toHaveAttribute("aria-checked", "false");
    await dndRow().click();
    await expect(dndRow()).toHaveAttribute("aria-checked", "true");
    await browser.keys("Escape");
    await indicator().waitForDisplayed();
    await expect(indicator()).toHaveAttribute("aria-label", "Do Not Disturb · manual");
    // The chip joins the status strip: the panel keeps its height and nothing wraps.
    expect(await userPanelLayout()).toEqual(before);
    expect(before.overflows).toBe(false);
  });

  it("collects a mention without a toast while the badge counts", async () => {
    await selectChannel("Lobby");
    await raidBot.run(`say --channel Raid "${USER} pull at eight"`);
    await browser.waitUntil(async () => (await unreadBadge("Raid")) === "1", {
      timeout: 15_000,
      timeoutMsg: "no unread badge 1 on Raid",
    });
    await sleep(1500);
    await expect(anyToast()).not.toBeExisting();
  });

  it("says the system state can't be read here", async () => {
    await openSettingsGroup("Chat & Notifications");
    await $("//p[contains(@class,'dnd-status')][contains(., 'Unavailable')]").waitForDisplayed();
    await browser.keys("Escape");
  });

  it("stays on after a restart", async () => {
    await app.restart();
    await indicator().waitForDisplayed({ timeout: 30_000 });
    await openQuickSettings();
    await expect(dndRow()).toHaveAttribute("aria-checked", "true");
    await dndRow().click();
    await expect(dndRow()).toHaveAttribute("aria-checked", "false");
    await browser.keys("Escape");
    await expect(indicator()).not.toBeExisting();
  });
});
