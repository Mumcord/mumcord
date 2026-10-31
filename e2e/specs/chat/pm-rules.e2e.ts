import { browser, expect, $ } from "@wdio/globals";
import { testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  closeMessagesOverview,
  finishWizard,
  joinChannel,
  openMessagesOverview,
  pmRow,
  ScriptBot,
  selectChannel,
  USER,
  waitMessage,
} from "../../helpers/chat-ui.ts";

const BOT = "StrangerBot";

/** Whether the private messages overview lists a conversation with the bot. */
async function hasConversation(): Promise<boolean> {
  await openMessagesOverview();
  const exists = await pmRow(BOT).isExisting();
  await closeMessagesOverview();
  return exists;
}

function privateHeader() {
  return $(`//h2[contains(@class,'chat-header__title') and contains(.,'${BOT}')]`);
}

function requestsEntry() {
  return $("[data-testid='message-requests-entry']");
}

async function openPrivacySettings(): Promise<void> {
  const button = $("button[aria-label='Quick settings']");
  await button.waitForClickable();
  await button.click();
  await $("//button[@role='menuitem'][contains(., 'All settings')]").click();
  await $("//button[@role='tab' and .='Privacy']").click();
}

// Issue #141: "Private messages from: Friends only" holds private messages from a standard client
// (the bot is not a Mumcord user) as a message request; accepting delivers them and accepts the
// conversation. Channel chat is never filtered. The bot shares the user's channel, so the
// "People in my current channel" exception is turned off.
describe("chat: private message rules and message requests (#141)", () => {
  let stranger: ScriptBot;

  before(async () => {
    await testenv.up("v15");
    await finishWizard();
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
    await selectChannel("Lobby");
    stranger = new ScriptBot({ name: BOT });
    await stranger.connected();
    await stranger.run("join Lobby");
  });

  after(() => {
    stranger.stop();
  });

  it("Settings → Privacy: choose Friends only and turn off the current-channel exception", async () => {
    await openPrivacySettings();
    const friendsOnly = $("[data-testid='pm-rule-friendsOnly']");
    await friendsOnly.waitForExist();
    // The radio is drawn as a card; click the card (its label).
    await $("//input[@data-testid='pm-rule-friendsOnly']/ancestor::label").click();
    await expect(friendsOnly).toBeSelected();
    const currentChannel = () =>
      $(
        "//input[@role='switch'][@id=//label[starts-with(.,'People in my current channel')]/@for]",
      );
    await currentChannel().waitForExist();
    await expect(currentChannel()).toBeSelected();
    await currentChannel().click();
    await expect(currentChannel()).not.toBeSelected();
    await browser.keys("Escape");
  });

  it("a private message from a standard client becomes a message request, not a chat", async () => {
    await stranger.run(`pm ${USER} "held hello"`);
    await requestsEntry().waitForDisplayed({ timeout: 15_000 });
    await expect(requestsEntry().$(".requests-entry__count")).toHaveText("1");
    expect(await hasConversation()).toBe(false);
    await requestsEntry().waitForClickable();
    await requestsEntry().click();
    const card = $(".request-card--messages");
    await card.waitForDisplayed();
    await browser.waitUntil(async () => (await card.getText()).includes("held hello"), {
      timeout: 10_000,
      timeoutMsg: "held message text not shown in the request",
    });
  });

  it("channel messages from the same person are not filtered", async () => {
    await selectChannel("Lobby");
    await stranger.run('say --channel Lobby "open channel hello"');
    await waitMessage("open channel hello");
    expect(await hasConversation()).toBe(false);
  });

  it("Accept delivers the held message; the next private message shows directly", async () => {
    await requestsEntry().click();
    const accept = $("[data-testid='request-accept']");
    await accept.waitForClickable();
    await accept.click();
    await privateHeader().waitForDisplayed({ timeout: 10_000 });
    await waitMessage("held hello");
    await requestsEntry().waitForExist({ reverse: true });

    await stranger.run(`pm ${USER} "direct hello"`);
    await waitMessage("direct hello");
    await expect(privateHeader()).toBeDisplayed();
    expect(await requestsEntry().isExisting()).toBe(false);
    await openMessagesOverview();
    await pmRow(BOT).waitForDisplayed();
    await closeMessagesOverview();
  });

  it("Revoke chat consent in the person menu turns the next private message into a request again", async () => {
    const person = $(`//div[contains(@class,'tree-person') and contains(.,'${BOT}')]`);
    await person.waitForDisplayed({ timeout: 10_000 });
    await person.click({ button: "right" });
    const revoke = $("//*[@role='menuitem'][normalize-space()='Revoke chat consent…']");
    await revoke.waitForClickable();
    await revoke.click();
    const dialog = $(`//dialog[.//*[contains(.,'Revoke chat consent for ${BOT}?')]]`);
    await dialog.waitForDisplayed();
    await dialog.$("//button[normalize-space()='Revoke']").click();
    await dialog.waitForDisplayed({ reverse: true });

    await stranger.run(`pm ${USER} "after revoke"`);
    await requestsEntry().waitForDisplayed({ timeout: 15_000 });
    await expect(requestsEntry().$(".requests-entry__count")).toHaveText("1");
  });
});
