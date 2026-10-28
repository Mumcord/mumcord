import { browser, expect, $ } from "@wdio/globals";
import { app, testenv } from "../../helpers/index.ts";
import { hasClass } from "../../helpers/selectors.ts";
import {
  addFirstServer,
  finishWizard,
  joinChannel,
  reconnectSaved,
  ScriptBot,
  selectChannel,
  USER,
  waitMessage,
} from "../../helpers/chat-ui.ts";

/** Certificate name of the bot: the same identity across both of its names. */
const CERT = "FriendQA";

function friendItem(name: string) {
  return $(`//section[contains(@class,'friends-list')]//button[starts-with(@aria-label,'${name},')]`);
}

async function friendStatus(name: string): Promise<string> {
  return (await friendItem(name).getAttribute("aria-label")) ?? "";
}

function presence() {
  return $(".chat-header__presence");
}

async function waitFriendStatus(name: string, status: RegExp): Promise<void> {
  await browser.waitUntil(async () => status.test(await friendStatus(name)), {
    timeout: 15_000,
    timeoutMsg: `friend ${name} never matched ${String(status)}`,
  });
}

// AC-1.54: friends by certificate hash with online state, kept private history (FR-1.64, FR-1.65).
// Automatic history deletion leaving friend chats alone is covered by the store tests: it only
// touches messages older than a day.
describe("chat: friends (AC-1.54)", () => {
  let friendBot: ScriptBot;

  before(async () => {
    await testenv.up("v15");
    await finishWizard();
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
    await selectChannel("Lobby");
    friendBot = new ScriptBot({ name: "FriendBot", cert: CERT });
    await friendBot.connected();
    await friendBot.run("join Lobby");
  });

  after(() => {
    friendBot.stop();
  });

  it("adds Bot as a friend from the person menu; it shows as online", async () => {
    const person = $(`//div[${hasClass('tree-person')} and contains(.,'FriendBot')]`);
    await person.waitForDisplayed({ timeout: 10_000 });
    await person.click({ button: "right" });
    const row = $("//*[@role='menuitemcheckbox'][starts-with(normalize-space(),'Friend')]");
    await row.waitForClickable();
    await row.click();
    await expect(row).toHaveAttribute("aria-checked", "true");
    await browser.keys("Escape");
    await waitFriendStatus("FriendBot", /Online in Lobby$/);
  });

  it("opens the private chat from the friends list with the online state in the header", async () => {
    await friendBot.run(`pm ${USER} "hello friend"`);
    await friendItem("FriendBot").click();
    await waitMessage("hello friend");
    await expect(presence()).toHaveText("Online");
  });

  it("Bot disconnects: offline in the list and in the private chat header", async () => {
    friendBot.stop();
    await waitFriendStatus("FriendBot", /Offline, last seen /);
    await browser.waitUntil(async () => (await presence().getText()).startsWith("Offline"), {
      timeoutMsg: "the private chat header still shows the friend as online",
    });
  });

  it("Bot reconnects with another name: still the same friend with the earlier history", async () => {
    friendBot = new ScriptBot({ name: "FriendBot2", cert: CERT });
    await friendBot.connected();
    await waitFriendStatus("FriendBot2", /^FriendBot2, Online/);
    expect(await friendItem("FriendBot").isExisting()).toBe(false);
    await selectChannel("Lobby");
    await friendItem("FriendBot2").click();
    await waitMessage("hello friend");
    await expect(presence()).toHaveText("Online");
  });

  it("keeps the friend and the private history after a restart", async () => {
    await app.restart();
    await reconnectSaved();
    await waitFriendStatus("FriendBot2", /Online/);
    await friendItem("FriendBot2").click();
    await waitMessage("hello friend");
  });
});
