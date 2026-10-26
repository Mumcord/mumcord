import { browser, expect, $ } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  clickButton,
  finishWizard,
  joinChannel,
  messageTexts,
  reconnectSaved,
  selectChannel,
  sleep,
  waitMessage,
} from "../../helpers/chat-ui.ts";

async function restartAndOpenLobby(): Promise<void> {
  await app.restart();
  await reconnectSaved();
  await joinChannel("Lobby");
  await selectChannel("Lobby");
}

async function openSettings(): Promise<void> {
  const button = $("button[aria-label='Quick settings']");
  await button.waitForClickable();
  await button.click();
  await $("//button[@role='menuitem'][contains(., 'All settings')]").click();
  await $("//button[@role='tab' and .='Chat & history']").click();
}

// AC-1.17: history survives a restart, "Clear history" removes it, "Save history" off keeps nothing.
describe("chat: local history (AC-1.17)", () => {
  before(async () => {
    await testenv.up("v15");
    await finishWizard();
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
    await selectChannel("Lobby");
  });

  it("shows received messages again after restart and reconnect", async () => {
    await bot(["join", "Lobby"], { name: "HistBot", hold: 1 });
    await bot(["say", "--channel", "Lobby", "history one"], { name: "HistBot", hold: 1 });
    await bot(["say", "--channel", "Lobby", "history two"], { name: "HistBot", hold: 1 });
    await waitMessage("history two");
    await restartAndOpenLobby();
    await waitMessage("history one");
    await waitMessage("history two");
  });

  it("Clear history removes the channel's messages (also after restart)", async () => {
    await clickButton("Clear history");
    await $("//div[@role='alertdialog']//button[.='Clear history']").click();
    await browser.waitUntil(async () => (await messageTexts()).length === 0, {
      timeoutMsg: "messages still shown after clearing",
    });
    await restartAndOpenLobby();
    await sleep(1500);
    const texts = await messageTexts();
    expect(texts.filter((t) => t.includes("history"))).toEqual([]);
  });

  it("with Save history off, new messages are not kept after restart", async () => {
    await openSettings();
    const toggle = () =>
      $("//input[@role='switch'][@id=//label[starts-with(.,'Save history on')]/@for]");
    await toggle().waitForExist();
    await expect(toggle()).toBeSelected();
    await toggle().click();
    await expect(toggle()).not.toBeSelected();
    await browser.keys("Escape");
    await bot(["say", "--channel", "Lobby", "volatile three"], { name: "HistBot", hold: 1 });
    await waitMessage("volatile three");
    await restartAndOpenLobby();
    await sleep(1500);
    const texts = await messageTexts();
    expect(texts.filter((t) => t.includes("volatile three"))).toEqual([]);
    // The switch itself must persist.
    await openSettings();
    await expect(toggle()).not.toBeSelected();
  });
});
