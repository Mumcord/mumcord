// AC-6.25 (spec 06): Mumbot Music against the local mumbot of testenv/mumbot. The install prompt
// marks the network and keychain permissions as high risk; "Configure bot" refuses plain http to
// a public address, rejects a wrong key and saves the right one; the player's skip, stop, volume,
// add by name/URL and upload reach the bot; the key never appears in the plugin's data export;
// when the bot leaves the user's channel the menu entry goes and the dialog says so.
import { $, browser, expect } from "@wdio/globals";
import type { ChainablePromiseElement } from "webdriverio";
import { app, registry } from "../../helpers/index.ts";
import * as mumbot from "../../helpers/mumbot.ts";
import { addFirstServer, finishWizard, joinChannel } from "../../helpers/chat-ui.ts";
import { openSettingsGroup, turnOnDeveloperMode } from "../../helpers/settings-ui.ts";
import * as testenv from "../../helpers/testenv.ts";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

const BOT = "MumbotLocal";

interface Track {
  title: string;
  source: string;
  channel: number;
}
interface QueueState {
  now_playing: Track | null;
  queue: Track[];
  volume: number;
}

const dialog = () => $("//div[@role='dialog' and contains(@class,'userpanel-modal__dialog')]");
const dialogButton = (text: string) =>
  dialog().$(`.//button[normalize-space(.)='${text}'][not(ancestor::div[contains(@class,'pr-row')])]`);
const field = (label: string) => dialog().$(`.//label[normalize-space(.)='${label}']/following::input[1]`);

function treePerson(name: string) {
  return $(`//div[contains(@class,'tree-person') and contains(.,'${name}')]`);
}

async function invoke<T>(command: string, args: object): Promise<T> {
  return browser.execute(
    (c: string, a: object) => {
      const w = window as unknown as {
        __TAURI_INTERNALS__: { invoke: (c: string, a: object) => Promise<unknown> };
      };
      return w.__TAURI_INTERNALS__.invoke(c, a);
    },
    command,
    args,
  ) as Promise<T>;
}

/**
 * Puts `text` into a field the way typing does (input event, then leaving the field). WebDriver
 * typing in this WebView sometimes drops Shift and turns ":" into ";", which breaks addresses.
 */
async function fill(input: ChainablePromiseElement, text: string): Promise<void> {
  await input.waitForDisplayed();
  await browser.execute(
    (el: HTMLInputElement, value: string) => {
      el.focus();
      // React tracks the value it last rendered; the prototype's setter goes around that.
      Reflect.set(HTMLInputElement.prototype, "value", value, el);
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.blur();
    },
    // Resolved first: only a found element crosses into the page as a DOM node.
    (await input.getElement()) as unknown as HTMLInputElement,
    text,
  );
}

async function openMenuOnBot(): Promise<void> {
  await treePerson(BOT).waitForDisplayed({ timeout: 20_000 });
  await treePerson(BOT).click({ button: "right" });
  await $("(//*[@role='menuitem'])[1]").waitForDisplayed();
}

async function configureBot(): Promise<void> {
  await openMenuOnBot();
  const item = $("//*[@role='menuitem'][normalize-space()='Configure bot']");
  await item.waitForClickable();
  await item.click();
  await dialog().waitForDisplayed();
}

/** Types the address (committed on blur) and stores the key through the password field's Save. */
async function enterSetup(address: string, key: string): Promise<void> {
  await fill(field("Bot address (IP or domain, with port)"), address);
  const keyInput = field("API key");
  await fill(keyInput, key);
  await dialog()
    .$(".//label[normalize-space(.)='API key']/following::button[normalize-space(.)='Save'][1]")
    .click();
  // The field empties once the key went to the keychain.
  await browser.waitUntil(async () => (await keyInput.getValue()) === "", {
    timeoutMsg: "the API key was not stored",
  });
}

async function testConnection(expected: string): Promise<void> {
  const test = dialogButton("Test connection");
  await test.waitForClickable({ timeout: 15_000 });
  await test.click();
  await expect(dialog()).toHaveText(expect.stringContaining(expected), { wait: 20_000 });
}

describe("plugins: Mumbot Music with a local mumbot (AC-6.25)", () => {
  let fixtures: registry.FixtureRegistry;
  let bot: mumbot.LocalMumbot;
  let urlTrack: mumbot.TrackServer | undefined;

  // The plugin polls with the same key every 3 s, two thirds of the bot's 30 requests per
  // minute. So the test waits on the plugin's own view and asks the bot itself only once per
  // step; more would make the plugin's clicks hit the rate limit.
  const queue = () => bot.api<QueueState>("GET", "/queue");
  const holds = (q: QueueState, source: string) =>
    [q.now_playing, ...q.queue].some((track) => track?.source === source);
  const shows = (text: string) =>
    expect(dialog()).toHaveText(expect.stringContaining(text), { wait: 20_000 });
  /** Every click is a request too: after a busy step, give the per-minute budget time. */
  const letTheRateLimitRecover = () => sleep(30_000);
  /** The plugin's queue list is labelled with its length. */
  const showsQueue = (count: number) =>
    dialog()
      .$(`ul[aria-label='Queue · ${String(count)}']`)
      .waitForExist({ timeout: 20_000 });
  /** "Add track" turns on once the plugin has the text, and the plugin empties the field when it sends the track. */
  async function addTrack(text: string): Promise<void> {
    const input = field("Add a file name or URL");
    await fill(input, text);
    const add = dialogButton("Add track");
    await add.waitForEnabled({ timeout: 10_000 });
    await add.click();
    await browser.waitUntil(async () => (await input.getValue()) === "", {
      timeoutMsg: `the plugin never sent ${text}`,
    });
  }

  before(async function () {
    this.timeout(20 * 60_000);
    await testenv.up("v15");
    fixtures = await registry.start();
    bot = await mumbot.start();
    await app.restart({ env: fixtures.env() });
    await finishWizard("Alice");
  });

  after(async () => {
    await urlTrack?.close();
    await bot.stop();
    await fixtures.stop();
  });

  it("marks the network and keychain permissions as high risk and installs", async () => {
    await turnOnDeveloperMode();
    await openSettingsGroup("Plugins");
    // The page keeps the scroll position of Advanced & Developer (tabs out of view), so take the
    // empty Installed tab's own way to Browse.
    await $("//button[normalize-space(.)='Browse plugins']").click();
    const install = $("button[aria-label='Install Mumbot Music']");
    await install.waitForClickable({ timeout: 30_000 });
    await install.click();
    const prompt = $("//div[contains(concat(' ',@class,' '),' plugin-prompt ')]");
    await prompt.waitForDisplayed({ timeout: 30_000 });
    for (const text of ["Talk to a server you choose", "Keep a secret"]) {
      const row = prompt.$(`.//li[.//span[contains(@class,'perm-text') and starts-with(normalize-space(.),'${text}')]]`);
      await expect(row.$(".perm-risk-badge")).toHaveText("High risk");
    }
    // The prompt checks the package first (progress bar), then offers Install.
    const confirm = prompt.$(".//button[normalize-space(.)='Install']");
    await confirm.waitForClickable({ timeout: 60_000 });
    await confirm.click();
    await expect($(".plugin-toast")).toHaveText(expect.stringContaining("Mumbot Music installed"), {
      wait: 30_000,
    });
    await browser.keys("Escape");
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
  });

  it("refuses public plain http, rejects a wrong key and saves the right one", async () => {
    await configureBot();
    await enterSetup(bot.address, "not-the-key");
    await testConnection("The API key was rejected");
    // 203.0.113.0/24 is reserved for documentation: the client refuses before sending anything.
    await enterSetup("http://203.0.113.5:8081", "not-the-key");
    await testConnection("Plain http is only allowed for local addresses");
    await enterSetup(bot.address, bot.key);
    await testConnection("Connection works");
    await dialogButton("Save").click();
    await expect(dialog()).toHaveText(expect.stringContaining("Nothing is playing"), { wait: 15_000 });
  });

  it("adds tracks, skips, changes the volume and stops", async () => {
    await addTrack("tone-a.mp3");
    await addTrack("tone-b.mp3");
    await addTrack("tone-a.mp3");
    await showsQueue(2);
    // Skip right away: each tone lasts 6 s, so only the skip shortens the queue this soon.
    await dialogButton("Skip").click();
    await sleep(1000);
    let q = await queue();
    expect(q.queue).toHaveLength(1);
    expect(q.now_playing).not.toBeNull();

    const slider = dialog().$("input[type='range']");
    await slider.click();
    await browser.keys(["Home"]);
    await sleep(1000);
    expect((await queue()).volume).toBe(0);

    await dialogButton("Stop").click();
    await sleep(1000);
    q = await queue();
    expect(q.now_playing).toBeNull();
    expect(q.queue).toHaveLength(0);
    await shows("Nothing is playing");
  });

  it("adds a track by URL", async () => {
    await letTheRateLimitRecover();
    urlTrack = await mumbot.serveTrack(mumbot.media("tone-a.mp3"), "from-url.mp3");
    // A 6 s tone first keeps the new track waiting in the queue while it is checked.
    await addTrack("tone-a.mp3");
    await addTrack(urlTrack.url);
    await showsQueue(1);
    expect(holds(await queue(), "url")).toBe(true);
    await dialogButton("Stop").click();
    await shows("Nothing is playing");
  });

  it("uploads an MP3", async () => {
    await letTheRateLimitRecover();
    await addTrack("tone-a.mp3");
    // The native file dialog cannot be driven over WebDriver: send what the picker sends after
    // the user chose a file (the host turns the path into a file handle for the upload).
    await invoke("plugin_ui_action", {
      id: "mumbot-music",
      slot: { kind: "dialog" },
      action: "upload",
      value: { type: "file", path: mumbot.media("tone-b.mp3") },
      serverId: null,
    });
    await showsQueue(1);
    expect(holds(await queue(), "upload")).toBe(true);
    await dialogButton("Stop").click();
    await shows("Nothing is playing");
  });

  it("keeps the API key out of the plugin's data export", async () => {
    const exported = await invoke<string>("plugin_export_data", { id: "mumbot-music" });
    expect(exported).not.toContain(bot.key);
  });

  it("drops the menu entry and says so in the dialog when the bot leaves the channel", async () => {
    // Three 6 s tones into Raid keep the bot there long enough; it returns to Lobby afterwards.
    for (const file of ["tone-a.mp3", "tone-b.mp3", "tone-a.mp3"]) {
      await bot.api("POST", "/play", { file, channel: mumbot.channelIds.Raid });
    }
    await expect(dialog()).toHaveText(expect.stringContaining("Mumbot is not in your channel"), {
      wait: 15_000,
    });
    await dialog().$(".userpanel-modal__close").click();
    await dialog().waitForExist({ reverse: true });
    await openMenuOnBot();
    await expect($("//*[@role='menuitem'][normalize-space()='Configure bot']")).not.toBeExisting();
    await browser.keys("Escape");
    await bot.api("POST", "/stop");
  });
});
