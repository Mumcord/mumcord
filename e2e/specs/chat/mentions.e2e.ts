import { browser, expect, $ } from "@wdio/globals";
import { app, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  composer,
  finishWizard,
  joinChannel,
  ScriptBot,
  selectChannel,
  USER,
  waitMessage,
} from "../../helpers/chat-ui.ts";

// FR-1.51 / AC-1.41: a mention highlights the message and notifies even while another chat is
// open; typing @ offers the people on the server and the sent text stays plain "@Name".
describe("chat: mentions (AC-1.41)", () => {
  let mentionBot: ScriptBot;

  before(async () => {
    await testenv.up("v15");
    await app.restart({});
    await finishWizard();
    await addFirstServer(64738);
    await joinChannel("Lobby");
    mentionBot = new ScriptBot({ name: "MentionBot" });
    await mentionBot.connected();
    await mentionBot.run("join Lobby");
  });

  after(() => {
    mentionBot.stop();
  });

  it("highlights a message that mentions the user and shows a toast", async () => {
    await selectChannel("Raid");
    await mentionBot.run(`say --channel Lobby "hey @${USER.toLowerCase()}, are you there?"`);
    const toast = $(
      "//section[@aria-label='Notifications']//button[contains(@class,'toast__open') and contains(.,'mentioned you')]",
    );
    await toast.waitForDisplayed({ timeout: 15_000 });
    await toast.click();
    await waitMessage("are you there?");
    await expect($("//li[contains(@class,'chat-message--mention')]")).toBeDisplayed();
  });

  it("does not highlight a message that only contains an email address", async () => {
    await mentionBot.run(`say --channel Lobby "mail ${USER.toLowerCase()}@example.com please"`);
    await waitMessage("please");
    const marked = await browser.execute(
      () => document.querySelectorAll(".chat-message--mention").length,
    );
    expect(marked).toBe(1);
  });

  it("offers people after @ and sends plain @Name", async () => {
    const box = composer();
    await box.waitForEnabled();
    await box.setValue("hello @Men");
    const option = $("//*[@role='listbox']//*[@role='option' and contains(.,'MentionBot')]");
    await option.waitForDisplayed({ timeout: 10_000 });
    await browser.keys(["Enter"]);
    expect(await box.getValue()).toBe("hello @MentionBot ");
    await browser.keys(["Enter"]);
    const got = await mentionBot.run(`expect message --from ${USER} --contains "@MentionBot"`);
    expect(got).toContain("@MentionBot");
  });
});
