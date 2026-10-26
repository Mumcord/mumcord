import { browser, expect, $ } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import {
  channelName,
  finishWizard,
  joinChannel,
  ScriptBot,
  selectChannel,
  sendText,
  sleep,
  trustIfAsked,
  waitMessage,
} from "../../helpers/chat-ui.ts";
import { button, dialogTitle, fill, serverButton, waitServerStatus } from "../../helpers/servers-ui.ts";

// Epic 2, several servers at once (AC-2.1–2.10), against the local test servers v15 ("Alpha"),
// v15b ("Bravo") and v14 ("Charlie"). The user is "Tester"; second clients are bots.
const USER = "Tester";
const ALPHA = "Alpha";
const BRAVO = "Bravo";
const CHARLIE = "Charlie";
const BOT = { timeout: 25 };

interface SettingsLike {
  maxConnectedServers: number;
}

async function addServer(label: string, svc: "v15" | "v15b" | "v14"): Promise<void> {
  await browser.$('button[aria-label="Add a server"]').click();
  await dialogTitle("Add a server").waitForDisplayed();
  await fill("Address", "127.0.0.1");
  await fill("Port", String(testenv.ports[svc]));
  await fill("Name in the server bar", label);
  await fill("Your name on this server", USER);
  await button("Add server").click();
  await serverButton(label).waitForDisplayed();
}

async function barLabel(name: string): Promise<string> {
  return (await serverButton(name).getAttribute("aria-label")) ?? "";
}

async function waitBarLabel(name: string, re: RegExp, timeout = 25_000): Promise<void> {
  await browser.waitUntil(async () => re.test(await barLabel(name)), {
    timeout,
    timeoutMsg: `bar label of ${name} never matched ${String(re)}, last "${await barLabel(name)}"`,
  });
}

/** Clicks the server (connects or shows it), trusting its certificate on first use. */
async function open(name: string): Promise<void> {
  await serverButton(name).click();
  await trustIfAsked();
}

async function connect(name: string): Promise<void> {
  await open(name);
  await waitServerStatus(name, "connected").catch(() => waitBarLabel(name, /\(connected\)/));
}

async function show(name: string): Promise<void> {
  await serverButton(name).click();
  await channelName("Lobby").waitForDisplayed({ timeout: 15_000 });
}

async function serverMenu(name: string, item: string): Promise<void> {
  await serverButton(name).click({ button: "right" });
  const row = $(`//*[@role='menuitem' or @role='menuitemcheckbox'][normalize-space()='${item}']`);
  await row.waitForClickable();
  await row.click();
}

async function setLimit(limit: number): Promise<void> {
  const current = await app.invoke<SettingsLike>("settings_get");
  await app.invoke("settings_update", { settings: { ...current, maxConnectedServers: limit } });
}

function toast(contains: string) {
  return $(
    `//section[@aria-label='Notifications']//button[contains(@class,'toast__open') and contains(.,'${contains}')]`,
  );
}

async function pageText(): Promise<string> {
  return browser.execute(() => document.body.innerText);
}

describe("multiple servers (AC-2.1–2.10)", function () {
  this.bail(true);

  let alphaBot: ScriptBot;
  let bravoBot: ScriptBot;

  before(async () => {
    await testenv.reset("v15");
    await testenv.reset("v15b");
    await testenv.reset("v14");
    await finishWizard(USER);
    await $("button[aria-label='Add a server']").waitForDisplayed({ timeout: 30_000 });
    await addServer(ALPHA, "v15");
    await addServer(BRAVO, "v15b");
    await addServer(CHARLIE, "v14");
    alphaBot = new ScriptBot({ name: "AlphaBot", port: testenv.ports.v15 });
    bravoBot = new ScriptBot({ name: "BravoBot", port: testenv.ports.v15b });
    await alphaBot.connected();
    await bravoBot.connected();
    await alphaBot.run("join Raid");
    await bravoBot.run("join Raid");
    await setLimit(2);
  });

  after(async () => {
    alphaBot.stop();
    bravoBot.stop();
    await testenv.up("v15b").catch(() => undefined);
  });

  it("AC-2.1: both servers connect; voice mark only on the joined one; B sees the user deafened", async () => {
    await connect(ALPHA);
    await connect(BRAVO);
    expect(await barLabel(ALPHA)).toBe("Alpha (connected)");
    expect(await barLabel(BRAVO)).toBe("Bravo (connected)");
    await show(ALPHA);
    await joinChannel("Lobby");
    await waitBarLabel(ALPHA, /voice server/);
    expect(await barLabel(BRAVO)).not.toContain("voice server");
    await bot(["expect", "user", USER, "--in", "Lobby", "--deaf", "false"], { ...BOT, name: "A1", port: testenv.ports.v15 });
    await bot(["expect", "user", USER, "--deaf", "true"], { ...BOT, name: "B1", port: testenv.ports.v15b });
  });

  it("AC-2.2: joining on B moves voice there; A shows the user deafened; panel names the server", async () => {
    await show(BRAVO);
    await joinChannel("Lobby");
    await waitBarLabel(BRAVO, /voice server/);
    expect(await barLabel(ALPHA)).not.toContain("voice server");
    await bot(["expect", "user", USER, "--in", "Lobby", "--deaf", "false"], { ...BOT, name: "B2", port: testenv.ports.v15b });
    await bot(["expect", "user", USER, "--in", "Lobby", "--deaf", "true"], { ...BOT, name: "A2", port: testenv.ports.v15 });
    await show(ALPHA);
    await expect($(".userpanel-voice")).toHaveText("Voice: Lobby · Bravo");
  });

  it("AC-2.3: a followed channel on A (voice on B) shows badge and toast; clicking it shows A's chat; reply works", async () => {
    await selectChannel("Raid");
    const follow = $("//button[contains(@class,'follow-toggle') and @aria-pressed]");
    await follow.click();
    await expect(follow).toHaveAttribute("aria-pressed", "true");
    await show(BRAVO);
    await alphaBot.run('say --channel Raid "pull at eight"');
    await waitBarLabel(ALPHA, /unread messages/);
    const t = toast("Alpha · #Raid");
    await t.waitForDisplayed({ timeout: 15_000 });
    await t.click();
    await waitMessage("pull at eight");
    expect(await barLabel(ALPHA)).not.toContain("unread");
    await sendText("on my way");
    await alphaBot.run("expect message --contains \"on my way\" --channel Raid");
  });

  it("AC-2.4: a mention on A counts on A's icon; muting A stops toasts but not badges", async () => {
    await show(BRAVO);
    await alphaBot.run(`say --channel Raid "hey @${USER.toLowerCase()} look"`);
    await waitBarLabel(ALPHA, /mentions: 1/);
    await serverMenu(ALPHA, "Mute notifications");
    await show(BRAVO);
    await $("//section[@aria-label='Notifications']//button[contains(@class,'toast__close')]")
      .click()
      .catch(() => undefined);
    await alphaBot.run('say --channel Raid "second message"');
    await sleep(4000);
    expect(await toast("second message").isExisting()).toBe(false);
    await waitBarLabel(ALPHA, /mentions: 1/);
    await serverMenu(ALPHA, "Mute notifications");
  });

  it("AC-2.9: a private message from a client on B arrives while viewing A", async () => {
    await show(ALPHA);
    await bravoBot.run(`pm ${USER} "psst from bravo"`);
    await browser.waitUntil(
      async () =>
        (await browser.$$("//*[contains(@aria-label,'on Bravo')]").length) > 0 ||
        (await pageText()).includes("BravoBot"),
      { timeout: 15_000, timeoutMsg: "no direct message entry naming Bravo" },
    );
    const entry = $("//*[contains(@aria-label,'BravoBot') and contains(@aria-label,'Bravo')]");
    await entry.waitForDisplayed({ timeout: 5_000 });
    await entry.click();
    await waitMessage("psst from bravo");
  });

  it("AC-2.5: with limit 2 and A, B connected, clicking C shows the limit message; limit 3 connects C", async () => {
    await serverButton(CHARLIE).click();
    await browser.waitUntil(async () => (await pageText()).includes("maximum of 2 servers"), {
      timeout: 15_000,
      timeoutMsg: "no limit message",
    });
    expect(await barLabel(CHARLIE)).not.toContain("(connected)");
    await setLimit(3);
    await connect(CHARLIE);
    await serverMenu(CHARLIE, "Disconnect");
    await waitBarLabel(CHARLIE, /^Charlie$|\(error\)|connection lost/, 15_000).catch(() => undefined);
    await setLimit(2);
  });

  it("AC-2.7: stopping B reconnects only B; A keeps working; restarting B reconnects", async () => {
    await testenv.stop("v15b");
    await waitBarLabel(BRAVO, /reconnecting|connection lost/, 40_000);
    expect(await barLabel(ALPHA)).toContain("(connected)");
    await bot(["expect", "user", USER], { ...BOT, name: "A7", port: testenv.ports.v15 });
    await testenv.start("v15b");
    await waitBarLabel(BRAVO, /\(connected\)/, 90_000);
  });

  it("AC-2.8: a friend on B shows as online on B in the friends list; Join moves voice to B", async () => {
    const friend = new ScriptBot({ name: "FriendyB", cert: "FriendyB", port: testenv.ports.v15b });
    try {
      await friend.connected();
      await show(BRAVO);
      await friend.run("join Raid");
      const person = $("//*[contains(@class,'tree-person') and contains(.,'FriendyB')]");
      await person.waitForDisplayed({ timeout: 15_000 });
      await person.click({ button: "right" });
      const row = $("//*[@role='menuitemcheckbox'][starts-with(normalize-space(),'Friend')]");
      await row.waitForClickable();
      await row.click();
      await browser.keys("Escape");
      await show(ALPHA);
      const item = $("//section[contains(@class,'friends-list')]//button[starts-with(@aria-label,'FriendyB,')]");
      await item.waitForDisplayed({ timeout: 15_000 });
      const label = (await item.getAttribute("aria-label")) ?? "";
      console.log(`[AC-2.8] friend row: ${label}`);
      expect(label).toMatch(/Raid.*Bravo|Bravo.*Raid/);
      await $("button[aria-label=\"Join FriendyB's channel\"]").click();
      await waitBarLabel(BRAVO, /voice server/);
      await bot(["expect", "user", USER, "--in", "Raid", "--deaf", "false"], { ...BOT, name: "B8", port: testenv.ports.v15b });
    } finally {
      friend.stop();
    }
  });

  it("AC-2.6/2.10: always-connect and last-connected servers reconnect read-only at start; rejoin prompt names A only", async () => {
    await show(ALPHA);
    await joinChannel("Lobby");
    await waitBarLabel(ALPHA, /voice server/);
    await setLimit(3);
    await serverMenu(CHARLIE, "Always connect at start");
    await app.restart();
    await waitBarLabel(ALPHA, /\(connected\)/, 60_000);
    await waitBarLabel(BRAVO, /\(connected\)/, 60_000);
    await waitBarLabel(CHARLIE, /\(connected\)/, 60_000);
    expect(await barLabel(ALPHA)).not.toContain("voice server");
    const text = await pageText();
    console.log(`[AC-2.10] rejoin text present: ${/last time/.test(text)}`);
    expect(text).toContain("You were in Lobby last time");
  });

  it("AC-2.6: with limit 2 start connects C plus one of A/B; marking more servers than the limit is refused", async () => {
    await setLimit(2);
    await app.restart();
    await waitBarLabel(CHARLIE, /\(connected\)/, 60_000);
    await sleep(5000);
    const connected = [(await barLabel(ALPHA)).includes("(connected)"), (await barLabel(BRAVO)).includes("(connected)")];
    console.log(`[AC-2.6] limit 2: A connected=${String(connected[0])}, B connected=${String(connected[1])}`);
    expect(connected.filter(Boolean)).toHaveLength(1);
    await serverMenu(ALPHA, "Always connect at start");
    await serverMenu(BRAVO, "Always connect at start");
    await browser.waitUntil(async () => (await pageText()).includes("Only 2 servers can be set"), {
      timeout: 10_000,
      timeoutMsg: "marking a third server was not refused with a message",
    });
  });
});
