import { browser, expect, $ } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  finishWizard,
  joinChannel,
  ScriptBot,
  selectChannel,
  sleep,
  USER,
  waitMessage,
} from "../../helpers/chat-ui.ts";

function toast(contains: string) {
  return $(
    `//section[@aria-label='Notifications']//button[contains(@class,'toast__open') and contains(.,'${contains}')]`,
  );
}

function anyToast() {
  return $("//section[@aria-label='Notifications']//button[contains(@class,'toast__open')]");
}

function followButton() {
  return $("//button[contains(@class,'follow-toggle') and @aria-pressed]");
}

// FR-1.48c / QA.16: a message in a followed channel while another chat is open shows an in-app
// toast; clicking it opens and focuses that chat. The chat on screen never gets a toast. (The
// container has no notification service, so a system notification falls back to the toast.)
describe("chat: message notifications (FR-1.48c)", () => {
  let raidBot: ScriptBot;

  before(async () => {
    await testenv.up("v15");
    await app.restart({});
    await finishWizard();
    await addFirstServer(64738);
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

  it("shows a toast for the followed channel and opens it on click", async () => {
    await selectChannel("Lobby");
    await raidBot.run('say --channel Raid "<b>pull</b> at eight"');
    const raidToast = toast("#Raid");
    await raidToast.waitForDisplayed({ timeout: 15_000 });
    // textContent: WebKit's innerText skips the line-clamped preview although it is drawn.
    expect(await raidToast.getProperty("textContent")).toContain("RaidBot: pull at eight");
    await raidToast.click();
    await $("//h2[contains(@class,'chat-header__title') and contains(.,'Raid')]").waitForDisplayed();
    await waitMessage("at eight");
    const focused = await browser.execute(() => document.activeElement?.tagName ?? "");
    expect(focused).toBe("TEXTAREA");
    await expect(anyToast()).not.toBeExisting();
  });

  it("shows no toast for the chat on screen", async () => {
    await selectChannel("Raid");
    await raidBot.run('say --channel Raid "already here"');
    await waitMessage("already here");
    await sleep(1500);
    await expect(anyToast()).not.toBeExisting();
  });
});
