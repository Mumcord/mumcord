import { browser, expect, $, $$ } from "@wdio/globals";
import { app, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  finishWizard,
  joinChannel,
  ScriptBot,
  selectChannel,
  USER,
  waitMessage,
} from "../../helpers/chat-ui.ts";

function bell() {
  return $("[data-testid='notification-bell']");
}

function centre() {
  return $("[data-testid='notification-centre']");
}

function entry(title: string) {
  return $(
    `//section[@data-testid='notification-centre']//button[@data-testid='centre-entry'][contains(., '${title}')]`,
  );
}

async function openCentre(): Promise<void> {
  if (await centre().isExisting()) return;
  await bell().click();
  await centre().waitForDisplayed();
}

// AC-3.74 (FR-3.105, FR-3.106): a mention, a reply and an invite (a message card sent privately;
// the test bot cannot start a watch party) each show in the notification centre with the server
// name; clicking one switches to its message; "Mark all as read" clears the badge.
describe("chat: notification centre (AC-3.74)", () => {
  let pingBot: ScriptBot;

  before(async () => {
    await testenv.up("v15");
    await app.restart({});
    await finishWizard();
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
    pingBot = new ScriptBot({ name: "PingBot", port: testenv.ports.v15 });
    await pingBot.connected();
    await pingBot.run("join Lobby");
    await selectChannel("Raid");
  });

  after(() => {
    pingBot.stop();
  });

  it("collects a mention, a reply and an invite with the server name", async () => {
    await pingBot.run(`say --channel Lobby "@${USER} pull at eight"`);
    await pingBot.run(
      `say --channel Lobby "<blockquote><b>${USER}</b>: lunch at noon?</blockquote>sure thing"`,
    );
    await pingBot.run(`pm ${USER} "[card title=Raid-night]Bring flasks[/card]"`);
    await browser.waitUntil(
      async () => (await bell().getAttribute("aria-label")) === "Notifications, 3 unread",
      { timeout: 15_000, timeoutMsg: "bell does not count three unread entries" },
    );
    await openCentre();
    await entry("PingBot mentioned you").waitForDisplayed();
    await entry("PingBot replied to your message").waitForDisplayed();
    await entry("PingBot sent you a card").waitForDisplayed();
    const servers = await $$(
      "//section[@data-testid='notification-centre']//span[contains(@class,'centre-entry__chips')]/span[1]",
    ).map((chip) => chip.getText());
    expect(servers).toHaveLength(3);
    expect(servers[0]).not.toBe("");
    expect(new Set(servers).size).toBe(1);
    await expect(entry("PingBot mentioned you")).toHaveText(expect.stringContaining("#Lobby"));
  });

  it("switches to the message when an entry is clicked", async () => {
    await openCentre();
    await entry("PingBot mentioned you").click();
    await expect(centre()).not.toBeExisting();
    await $(
      "//h2[contains(@class,'chat-header__title') and contains(normalize-space(.),'Lobby')]",
    ).waitForDisplayed();
    await waitMessage("pull at eight");
  });

  it("clears the badge with Mark all as read", async () => {
    await selectChannel("Raid");
    await openCentre();
    await $("//button[.='Mark all as read']").click();
    await browser.waitUntil(
      async () => (await bell().getAttribute("aria-label")) === "Notifications",
      { timeout: 5_000, timeoutMsg: "badge not cleared" },
    );
    await expect($("[data-testid='notification-badge']")).not.toBeExisting();
    await browser.keys("Escape");
    await expect(centre()).not.toBeExisting();
  });
});
