// AC-15.1 – AC-15.7 (spec 15 §11, issue #872): "Forget local mutes when Mumcord starts".
// One global switch on Settings → Chat & Notifications. With it on, every stored local mute of
// every server is cleared exactly once per app start, before the first connect; volumes, share
// volumes, blocks and friends stay. Reconnects and server switches never clear, and turning the
// switch on clears nothing right away.
//
// Instance A (user "MuteQA") on the local test servers v15 ("Alpha") and v15b ("Bravo"); the
// second client is a mumbot script bot per person, each with a certificate of its own so the app
// recognises it again after a reconnect. The app records the speakers to a file, so "locally
// muted" (silent) and "cleared" (audible) are measured instead of assumed.
//
//   npm run e2e -- --spec e2e/specs/chat/forget-local-mutes.e2e.ts
import { execFileSync } from "node:child_process";
import { join } from "node:path";
import { browser, expect, $ } from "@wdio/globals";
import { app, testenv } from "../../helpers/index.ts";
import { openMembersTab } from "../../helpers/members-ui.ts";
import {
  finishWizard,
  joinChannel,
  OutputWav,
  ScriptBot,
  silentWav,
  sleep,
} from "../../helpers/chat-ui.ts";
import { artifactsDir, repoRoot } from "../../helpers/paths.ts";
import { personRow } from "../../helpers/voice-ui.ts";
import { openSettingsGroup, searchSettings, settingsCard } from "../../helpers/settings-ui.ts";
import { button, dialogTitle, fill, serverButton, waitServerStatus } from "../../helpers/servers-ui.ts";

const USER = "MuteQA";
/** Server A and server B: mutes are stored per server, the sweep must clear both at once. */
const ALPHA = "Alpha";
const BRAVO = "Bravo";
const MUTED = "MuteBot";
const BLOCKED = "BlockBot";
const FRIEND = "FriendBot";
const OPTION_ON = "Forget local mutes when Mumcord starts";
const speech = join(repoRoot, "testenv", "audio", "speech.wav");

const log = (msg: string): void => {
  console.log(`[15] ${msg}`);
};

/** The app's speakers, recorded through the `file:` audio backend. */
const out = new OutputWav(join(artifactsDir, "forget-local-mutes-out.wav"));

interface Snapshot {
  users: { session: number; name: string; channelId: number }[];
}

interface PersonSettingRow {
  volume: number;
  localMute: boolean;
  shareVolume: number;
}

// --- servers and connection (AC-15.5 needs two servers) ------------------------------------

async function addServer(label: string, svc: "v15" | "v15b"): Promise<void> {
  await $('button[aria-label="Add a server"]').click();
  await dialogTitle("Add a server").waitForDisplayed();
  await fill("Address", "127.0.0.1");
  await fill("Port", String(testenv.ports[svc]));
  await fill("Name in the server bar", label);
  await fill("Your name on this server", USER);
  await button("Add server").click();
  await serverButton(label).waitForDisplayed();
}

function barLabel(name: string): Promise<string> {
  return serverButton(name).getAttribute("aria-label").then((label) => label ?? "");
}

/** Clicks the server (connects it or just shows it), trusting its certificate on first use. */
async function open(name: string): Promise<void> {
  await serverButton(name).click();
  const trust = $("//button[.='Trust and connect']");
  if (await trust.waitForDisplayed({ timeout: 15_000 }).catch(() => false)) await trust.click();
}

async function connect(name: string): Promise<void> {
  await open(name);
  await waitServerStatus(name, "connected");
}

/** Joins Lobby on the shown server, so a locally muted person is not only an icon but silence. */
async function joinLobby(): Promise<void> {
  await joinChannel("Lobby");
  const deafen = $("button[aria-label='Deafen']");
  await browser.waitUntil(async () => (await deafen.getAttribute("aria-pressed")) !== "true", {
    timeout: 15_000,
    timeoutMsg: "still deafened after joining Lobby",
  });
  // Join/leave tones and toasts would otherwise land in the next measurement window.
  await sleep(1500);
}

// --- the person's local state in the channel tree ------------------------------------------

async function localMuted(name: string): Promise<boolean> {
  return (await personRow(name).$(".tree-person__name").getAttribute("data-local-muted")) === "true";
}

async function waitLocalMuted(name: string, muted: boolean): Promise<void> {
  await browser.waitUntil(async () => (await localMuted(name)) === muted, {
    timeout: 20_000,
    timeoutMsg: `${name} is ${muted ? "not" : "still"} shown as locally muted`,
  });
}

function mutedIcon(name: string) {
  return personRow(name).$("svg.person-icon--local-muted");
}

/**
 * Opens the person menu with a script contextmenu event. A WebDriver right-click makes
 * WebKitWebDriver type ":" as ";" for the rest of the session (see theme-language.e2e.ts), which
 * would break the settings search and the add-server form of this spec.
 */
async function openPersonMenu(name: string): Promise<void> {
  await browser.execute((person: string) => {
    const row = Array.from(document.querySelectorAll<HTMLElement>(".tree-person")).find(
      (element) => element.querySelector(".tree-person__name")?.firstChild?.textContent === person,
    );
    const box = row?.getBoundingClientRect();
    if (!row || !box) throw new Error(`no tree row for ${person}`);
    row.dispatchEvent(
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        button: 2,
        clientX: box.left + 8,
        clientY: box.top + 8,
      }),
    );
  }, name);
  await $("div[role='menu'].person-menu").waitForDisplayed({ timeout: 10_000 });
}

/** The local-mute switch row of the open person menu (the menu stays open while it toggles). */
function muteMenuItem() {
  return $(
    "//div[contains(@class,'person-menu')]//button[@role='menuitemcheckbox'][contains(@class,'quick-settings__row')]",
  );
}

async function setLocalMute(name: string, muted: boolean): Promise<void> {
  await openPersonMenu(name);
  const item = muteMenuItem();
  await item.waitForClickable();
  expect(await item.getAttribute("aria-checked")).toBe(String(!muted));
  await item.click();
  await browser.waitUntil(async () => (await item.getAttribute("aria-checked")) === String(muted), {
    timeoutMsg: `the local mute of ${name} did not switch`,
  });
  await browser.keys("Escape");
  await $("div[role='menu'].person-menu").waitForDisplayed({ reverse: true });
  await waitLocalMuted(name, muted);
}

async function blockPerson(name: string): Promise<void> {
  await openPersonMenu(name);
  const block = $("[data-testid='person-block']");
  await block.waitForClickable();
  await block.click();
  const dialog = $(`//dialog[.//h2[normalize-space()='Block ${name}?']]`);
  await dialog.waitForDisplayed();
  await $("[data-testid='block-confirm']").click();
  await browser.waitUntil(async () => personRow(name).$(".blocked-badge").isExisting(), {
    timeout: 15_000,
    timeoutMsg: `${name} is not marked as blocked`,
  });
}

/** A mumbot script client per person: its certificate makes it the same person after a reconnect. */
const bots: ScriptBot[] = [];

function startBot(name: string, svc: "v15" | "v15b" = "v15"): ScriptBot {
  const bot = new ScriptBot({ name, cert: name, port: testenv.ports[svc] });
  bots.push(bot);
  return bot;
}

async function addFriend(name: string): Promise<void> {
  await openPersonMenu(name);
  const add = $(
    "//div[contains(@class,'person-menu')]//button[@role='menuitem'][normalize-space()='Add friend']",
  );
  await add.waitForClickable();
  await add.click();
  await browser.keys("Escape");
  await $("div[role='menu'].person-menu").waitForDisplayed({ reverse: true });
}

/** Opens the full profile card of `name` with the local settings (volume, mute) disclosed. */
async function openProfileCard(name: string): Promise<void> {
  await personRow(name).click();
  const full = $("//div[@id='profile-preview']//button[@aria-label='Full profile']");
  await full.waitForDisplayed();
  await full.click();
  await $("dialog.profile-card").waitForDisplayed();
  await $("dialog.profile-card .disclosure__toggle").click();
  await $("dialog.profile-card .disclosure__panel").waitForDisplayed();
}

/** The session of `name` on the voice server, for the one setting without a UI (share volume). */
async function sessionOf(name: string): Promise<number> {
  const serverId = await app.invoke<number | null>("voice_server_get");
  if (serverId === null) throw new Error("no voice server");
  const snapshot = await app.invoke<Snapshot>("session_snapshot", { serverId });
  const session = snapshot.users.find((user) => user.name === name)?.session;
  if (session === undefined) throw new Error(`${name} is not on the voice server`);
  return session;
}

/**
 * A person's stored settings. The share volume has no UI without a running share, so the store of
 * the app says whether the sweep left it alone (AC-15.4).
 */
function personSettings(): PersonSettingRow[] {
  const db = join(app.profile(), "data", "app.mumcord.client", "mumcord.sqlite3");
  const rows = execFileSync(
    "sqlite3",
    [db, "select volume, local_mute, share_volume from person_setting;"],
    { encoding: "utf8" },
  ).trim();
  if (rows === "") return [];
  return rows.split("\n").map((line) => {
    const [volume, localMute, shareVolume] = line.split("|").map(Number);
    return { volume, localMute: localMute === 1, shareVolume };
  });
}

/**
 * Speaks once into the recording. A restarted app starts a new output file (the test audio backend
 * keeps one file per process), so the graded measurement must not race that first write.
 */
async function warmUpAudio(bot: ScriptBot): Promise<void> {
  await bot.run(`play ${speech}`, 20_000);
  await sleep(1500);
}

/** Peak of the app's speakers while `bot` talks in the user's channel. */
async function peakWhileTalking(bot: ScriptBot): Promise<number> {
  const before = out.length();
  await bot.run(`play ${speech}`, 20_000);
  await sleep(1000);
  const after = out.length();
  expect(after - before).toBeGreaterThan(48_000 * 6);
  return out.peakDb(before, after);
}

// --- settings steps -------------------------------------------------------------------------

/** One switch row of the "Local mutes" group (the group is above Blocked users). */
function forgetSwitch(name = OPTION_ON) {
  return settingsCard("Local mutes").$(
    `.//div[contains(@class,'settings-switch')][.//label[normalize-space()='${name}']]//input[@role='switch']`,
  );
}

async function openLocalMutes(): Promise<void> {
  await openSettingsGroup("Chat & Notifications");
  const card = settingsCard("Local mutes");
  await card.waitForDisplayed({ timeout: 10_000 });
  await forgetSwitch().scrollIntoView();
}

async function closeSettings(): Promise<void> {
  await browser.execute(() => {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
  });
  const closed = async (): Promise<boolean> =>
    $(".overlay__dialog")
      .waitForDisplayed({ timeout: 3_000, reverse: true })
      .then(() => true)
      .catch(() => false);
  for (let attempt = 0; attempt < 3; attempt++) {
    await browser.keys("Escape");
    if (await closed()) return;
  }
  await $(".overlay__dialog").waitForDisplayed({ reverse: true });
}

async function setForgetLocalMutes(on: boolean): Promise<void> {
  await openLocalMutes();
  const toggle = forgetSwitch();
  if ((await toggle.isSelected()) !== on) await toggle.click();
  await browser.waitUntil(async () => (await toggle.isSelected()) === on, {
    timeoutMsg: `"${OPTION_ON}" did not switch ${on ? "on" : "off"}`,
  });
  await closeSettings();
}

/** The search result group of a top-level settings page, as in the settings-search spec. */
function resultGroup(group: string) {
  return $(
    `//*[contains(@class,'settings-search__results')]//section[.//h3[contains(@class,'settings-search__group')][starts-with(normalize-space(.),'${group}')]]`,
  );
}

async function searchFindsOption(query: string): Promise<string[]> {
  await searchSettings(query);
  const group = resultGroup("Chat & Notifications");
  await group.waitForDisplayed({ timeout: 10_000 });
  return group.$$("button.settings-search__result").map((row) => row.getText());
}

function clearSearch(): Promise<void> {
  return $("input.settings-search__input")
    .click()
    .then(() => browser.keys(["Escape"]));
}

describe("chat: forget local mutes on start (AC-15.1 – AC-15.7)", function () {
  this.bail(true);
  let muteBot: ScriptBot;
  let blockBot: ScriptBot;
  let friendBot: ScriptBot;

  before(async () => {
    await testenv.reset("v15");
    await testenv.reset("v15b");
    await app.restart({
      audio: `file:${silentWav("forget-mutes-silence.wav", 600)}:${out.path}`,
    });
    // The rich sound theme's long tones would spill into the measurement windows.
    await app.useClassicSounds();
    await finishWizard(USER);
    await $("button[aria-label='Add a server']").waitForDisplayed({ timeout: 30_000 });
    await addServer(ALPHA, "v15");
    await addServer(BRAVO, "v15b");
    await connect(ALPHA);
    await joinLobby();
    muteBot = startBot(MUTED);
    await muteBot.connected();
    await muteBot.run("join Lobby");
    await personRow(MUTED).waitForDisplayed({ timeout: 20_000 });
    await warmUpAudio(muteBot);
  });

  after(async () => {
    for (const bot of bots) bot.stop();
    // AC-15.6 restarts v15; leave it running for the specs after this one.
    await testenv.up("v15").catch(() => undefined);
  });

  it("AC-15.1: the group sits above Blocked users, off by default, and the search finds it", async () => {
    await openLocalMutes();
    const card = settingsCard("Local mutes");
    await expect(card).toHaveText(expect.stringContaining(OPTION_ON));
    await expect(card).toHaveText(
      expect.stringContaining("People you muted only for yourself become audible again"),
    );
    // AC-15.1: off by default on a fresh profile.
    expect(await forgetSwitch().isSelected()).toBe(false);
    // The helper text is announced with the switch (§5).
    const describedBy = await forgetSwitch().getAttribute("aria-describedby");
    expect(describedBy).not.toBeNull();
    expect(await $(`//*[@id='${String(describedBy)}']`).getText()).toContain(
      "Volumes and blocked users are kept",
    );
    // Directly above the Blocked users group (§5, Q2.1).
    const [local, blocked] = [
      await card.getLocation(),
      await settingsCard("Blocked users").getLocation(),
    ];
    expect(local.y).toBeLessThan(blocked.y);

    for (const query of ["mute", "forget"]) {
      const results = await searchFindsOption(query);
      log(`settings search "${query}": ${JSON.stringify(results)}`);
      expect(results.some((text) => text.startsWith(OPTION_ON))).toBe(true);
    }
    await clearSearch();
    await closeSettings();
  });

  it("AC-15.2: with the option off, the local mute survives a restart (icon and silence)", async () => {
    await setLocalMute(MUTED, true);
    expect(await mutedIcon(MUTED).isExisting()).toBe(true);
    const muted = await peakWhileTalking(muteBot);
    log(`AC-15.2 peak while muted: ${muted.toFixed(1)} dBFS`);
    expect(muted).toBeLessThan(-60);

    await app.restart();
    await connect(ALPHA);
    await joinLobby();
    await personRow(MUTED).waitForDisplayed({ timeout: 20_000 });
    await waitLocalMuted(MUTED, true);
    expect(await mutedIcon(MUTED).isExisting()).toBe(true);
    await warmUpAudio(muteBot);
    const afterRestart = await peakWhileTalking(muteBot);
    log(`AC-15.2 peak after the restart: ${afterRestart.toFixed(1)} dBFS`);
    expect(afterRestart).toBeLessThan(-60);
  });

  it("AC-15.7: switching the option on clears nothing right away", async () => {
    await setForgetLocalMutes(true);
    await waitLocalMuted(MUTED, true);
    expect(await mutedIcon(MUTED).isExisting()).toBe(true);
    const stillMuted = await peakWhileTalking(muteBot);
    log(`AC-15.7 peak right after switching the option on: ${stillMuted.toFixed(1)} dBFS`);
    expect(stillMuted).toBeLessThan(-60);
  });

  it("AC-15.3: with the option on, the next start clears the mute (audible, no icon)", async () => {
    await app.restart();
    await connect(ALPHA);
    await joinLobby();
    await personRow(MUTED).waitForDisplayed({ timeout: 20_000 });
    await waitLocalMuted(MUTED, false);
    expect(await mutedIcon(MUTED).isExisting()).toBe(false);
    await warmUpAudio(muteBot);
    const audible = await peakWhileTalking(muteBot);
    log(`AC-15.3 peak after the cleared start: ${audible.toFixed(1)} dBFS`);
    expect(audible).toBeGreaterThan(-45);
  });

  it("AC-15.4: volumes, share volume, block and friend stay; only the mute is gone", async () => {
    await openProfileCard(MUTED);
    const slider = $(".profile-card__volume input[type='range']");
    await slider.waitForDisplayed();
    await slider.click();
    await browser.keys("Home");
    for (let i = 0; i < 10; i++) await browser.keys("ArrowRight");
    await expect($(".profile-card__volume output")).toHaveText("50 %");
    await browser.keys("Escape");
    // The share volume has no UI without a running share: set it through the same engine call the
    // share viewer uses, so it is stored per person like the voice volume.
    const serverId = await app.invoke<number | null>("voice_server_get");
    expect(serverId).not.toBeNull();
    await app.invoke("person_set_share_volume", {
      serverId,
      session: await sessionOf(MUTED),
      volume: 40,
    });

    blockBot = startBot(BLOCKED);
    await blockBot.connected();
    await blockBot.run("join Lobby");
    await personRow(BLOCKED).waitForDisplayed({ timeout: 20_000 });
    await blockPerson(BLOCKED);

    friendBot = startBot(FRIEND);
    await friendBot.connected();
    await friendBot.run("join Lobby");
    await personRow(FRIEND).waitForDisplayed({ timeout: 20_000 });
    await addFriend(FRIEND);

    await setLocalMute(MUTED, true);
    await app.restart();
    await connect(ALPHA);
    await joinLobby();
    await personRow(MUTED).waitForDisplayed({ timeout: 20_000 });

    // The mute is gone …
    await waitLocalMuted(MUTED, false);
    // … the volume is still 50 % …
    await openProfileCard(MUTED);
    await expect($(".profile-card__volume output")).toHaveText("50 %");
    await browser.keys("Escape");
    // … the block is still there …
    await expect(personRow(BLOCKED).$(".blocked-badge")).toBeExisting();
    // … the friend is still a friend …
    await openMembersTab();
    const friend = $(
      `//section[contains(@class,'friends-list')]//button[starts-with(@aria-label,'${FRIEND},')]`,
    );
    await friend.waitForDisplayed({ timeout: 15_000 });
    // … and the share volume survived in the same stored row as the cleared mute.
    const row = personSettings().find((setting) => setting.volume === 50);
    log(`AC-15.4 stored row: ${JSON.stringify(row)}`);
    expect(row).toBeDefined();
    expect(row?.shareVolume).toBe(40);
    expect(row?.localMute).toBe(false);
  });

  it("AC-15.5: a start with only server B connected clears server A's mute as well", async () => {
    await setLocalMute(MUTED, true);
    await app.restart();
    // Nothing auto-connects: the user connects B only.
    await serverButton(BRAVO).waitForDisplayed({ timeout: 30_000 });
    await sleep(2000);
    log(`AC-15.5 after the restart: ${ALPHA} "${await barLabel(ALPHA)}", ${BRAVO} "${await barLabel(BRAVO)}"`);
    expect(await barLabel(ALPHA)).not.toContain("(connected)");
    expect(await barLabel(BRAVO)).not.toContain("(connected)");
    // The sweep runs before any connect: no stored mute is left on any server.
    expect(personSettings().filter((setting) => setting.localMute)).toEqual([]);
    await connect(BRAVO);
    await open(ALPHA);
    await personRow(MUTED).waitForDisplayed({ timeout: 20_000 });
    await waitLocalMuted(MUTED, false);
    expect(await mutedIcon(MUTED).isExisting()).toBe(false);
  });

  it("AC-15.6: a forced reconnect while the app runs keeps the mute", async () => {
    await connect(ALPHA);
    await joinLobby();
    await personRow(MUTED).waitForDisplayed({ timeout: 20_000 });
    await setLocalMute(MUTED, true);
    await testenv.restart("v15");
    await waitServerStatus(ALPHA, "connected", 90_000);
    // The server restart dropped the bot too: the same certificate reconnects as the same person.
    muteBot.stop();
    muteBot = startBot(MUTED);
    await muteBot.connected();
    await muteBot.run("join Lobby");
    await personRow(MUTED).waitForDisplayed({ timeout: 20_000 });
    await waitLocalMuted(MUTED, true);
    expect(await mutedIcon(MUTED).isExisting()).toBe(true);
    log("AC-15.6 the mute set in this session survived the reconnect");
  });
});
