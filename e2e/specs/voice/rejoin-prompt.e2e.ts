// FR-1.14 / QA.9: quitting Mumcord while joined to a channel for voice makes the next fresh
// connect after the restart offer "You were in ‹channel› last time – Rejoin?". Rejoin is an
// explicit join (voice on in that channel); "Not now" keeps the read-only state. The offer comes
// once per app start (covered by the engine live test), and not at all when the user was not
// joined for voice at quit.
import { $, browser, expect } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import { addFirstServer, channelName, finishWizard, reconnectSaved } from "../../helpers/chat-ui.ts";
import { doubleClickRow, ownChannel } from "../../helpers/voice-ui.ts";

const ME = "RejoinQA";
const PORT = testenv.ports.v14;
const WATCH = { name: "RjWatcher", port: PORT, timeout: 30 };

const prompt = () => $("[role='status'][aria-label='Rejoin last channel'] .servers-status");
const promptButton = (text: string) =>
  $(`//div[@aria-label='Rejoin last channel']//button[normalize-space(.)='${text}']`);
const deafenButton = () => $("button[aria-label='Deafen']");

async function deafened(): Promise<boolean> {
  return (await deafenButton().getAttribute("aria-pressed")) === "true";
}

async function joinRaid(): Promise<void> {
  await doubleClickRow(channelName("Raid"));
  await bot(["expect", "user", ME, "--in", "Raid", "--deaf", "false"], WATCH);
  await browser.waitUntil(async () => (await ownChannel()) === "Raid", {
    timeout: 15_000,
    timeoutMsg: "app tree does not show the user in Raid",
  });
}

/** Restarts the app and connects freshly; the user lands read-only outside Raid. */
async function restartAndConnect(): Promise<void> {
  await app.restart();
  await reconnectSaved();
  await browser.waitUntil(deafened, { timeout: 15_000, timeoutMsg: "not read-only" });
  await bot(["expect", "user", ME, "--in", "/", "--deaf", "true"], WATCH);
}

describe("rejoin prompt after an app restart (FR-1.14, QA.9)", () => {
  before(async () => {
    await testenv.up("v14");
    await app.restart();
  });

  it("no prompt on the first connect; joining Raid for voice", async () => {
    await finishWizard(ME);
    await addFirstServer(PORT);
    await browser.waitUntil(deafened, { timeout: 15_000, timeoutMsg: "not read-only" });
    expect(await prompt().isExisting()).toBe(false);
    await joinRaid();
  });

  it("quit while in Raid → restart: the prompt offers Raid; Rejoin joins it with voice on", async () => {
    await restartAndConnect();
    await prompt().waitForDisplayed({ timeout: 15_000 });
    expect(await prompt().getText()).toContain("You were in Raid last time – Rejoin?");
    expect(await ownChannel()).not.toBe("Raid");
    await promptButton("Rejoin").click();
    await bot(["expect", "user", ME, "--in", "Raid", "--deaf", "false"], WATCH);
    await browser.waitUntil(async () => !(await deafened()), {
      timeout: 15_000,
      timeoutMsg: "still deafened after Rejoin",
    });
    await prompt().waitForExist({ reverse: true, timeout: 10_000 });
  });

  it("quit while in Raid → restart: Not now keeps the read-only state", async () => {
    await restartAndConnect();
    await prompt().waitForDisplayed({ timeout: 15_000 });
    await promptButton("Not now").click();
    await prompt().waitForExist({ reverse: true, timeout: 10_000 });
    expect(await deafened()).toBe(true);
    await bot(["expect", "user", ME, "--in", "/", "--deaf", "true"], WATCH);
  });

  it("not joined for voice at quit (read-only after Not now) → restart: no prompt", async () => {
    await restartAndConnect();
    await browser.pause(1_000);
    expect(await prompt().isExisting()).toBe(false);
  });
});
