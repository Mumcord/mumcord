import { browser, expect, $ } from "@wdio/globals";
import { app, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  finishWizard,
  joinChannel,
  reconnectSaved,
  ScriptBot,
  USER,
} from "../../helpers/chat-ui.ts";

const BOT = "PenPal";
// A named certificate keeps the conversation (and its pin and mute) across reconnects.
const CERT = "PenPalQA";

function row() {
  return $(`//button[@data-testid='pm-row'][starts-with(@aria-label,'${BOT},')]`);
}

function menuItem(name: string) {
  return $(`//div[@role='menu']//button[@role='menuitem'][contains(.,'${name}')]`);
}

async function openOverview(): Promise<void> {
  const button = $("[data-testid='pm-button']");
  await button.waitForClickable({ timeout: 15_000 });
  await button.click();
  await $("[data-testid='pm-overview']").waitForDisplayed();
}

/** Opens the conversation menu from the keyboard (Shift+F10) and picks `name`. */
async function chooseFromMenu(name: string): Promise<void> {
  // Focus only: a click would open the chat and read it.
  await browser.execute((el: HTMLElement) => {
    el.focus();
  }, await row());
  await browser.keys(["Shift", "F10"]);
  await menuItem(name).waitForDisplayed();
  await menuItem(name).click();
}

async function labelOfRow(): Promise<string> {
  return (await row().getAttribute("aria-label")) ?? "";
}

// Issue #173: the private messages overview lists conversations across servers with preview and
// unread count; pin and mute come from the conversation menu and survive a restart.
describe("chat: private messages overview (#173)", () => {
  let penPal: ScriptBot;

  before(async () => {
    await testenv.up("v15");
    await finishWizard();
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
    penPal = new ScriptBot({ name: BOT, cert: CERT });
    await penPal.connected();
  });

  after(() => {
    penPal.stop();
  });

  it("an incoming private message shows in the overview with its preview and unread count", async () => {
    await penPal.run(`pm ${USER} "<b>LAN party</b> on Saturday"`);
    await openOverview();
    await row().waitForDisplayed({ timeout: 15_000 });
    await browser.waitUntil(async () => (await labelOfRow()).includes("1 unread"), {
      timeout: 10_000,
      timeoutMsg: "unread count not in the row",
    });
    expect(await labelOfRow()).toContain("LAN party on Saturday");
    expect(await labelOfRow()).toContain("online");
    // Server HTML shows as plain text only.
    expect(await row().$("b").isExisting()).toBe(false);
  });

  it("Pin to top and Mute for 1 hour from the conversation menu", async () => {
    await chooseFromMenu("Pin to top");
    await $("//section[@aria-label='Pinned']").waitForDisplayed();
    await chooseFromMenu("Mute conversation");
    await menuItem("For 1 hour").click();
    await browser.waitUntil(async () => (await labelOfRow()).includes("muted"), {
      timeout: 10_000,
      timeoutMsg: "row not marked as muted",
    });
    expect(await labelOfRow()).toContain("pinned");
  });

  it("Mark as read clears the unread count", async () => {
    await chooseFromMenu("Mark as read");
    await browser.waitUntil(async () => !(await labelOfRow()).includes("unread"), {
      timeout: 10_000,
      timeoutMsg: "unread count not cleared",
    });
  });

  it("pin and mute survive a restart; the row opens the private chat", async () => {
    await app.restart({});
    await reconnectSaved();
    await openOverview();
    await row().waitForDisplayed({ timeout: 15_000 });
    const label = await labelOfRow();
    expect(label).toContain("pinned");
    expect(label).toContain("muted");
    await row().click();
    await $(`//h2[contains(@class,'chat-header__title') and contains(.,'${BOT}')]`).waitForDisplayed(
      { timeout: 10_000 },
    );
  });

  it("Start a new conversation picks someone on a connected server", async () => {
    await $("[data-testid='pm-new']").click();
    const person = $(`//button[@data-testid='pm-new-person'][contains(.,'${BOT}')]`);
    await person.waitForClickable();
    await person.click();
    await expect(
      $(`//h2[contains(@class,'chat-header__title') and contains(.,'${BOT}')]`),
    ).toBeDisplayed();
  });
});
