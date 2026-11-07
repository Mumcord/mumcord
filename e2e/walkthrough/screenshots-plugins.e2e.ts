// Screenshots for the 0.7.0 docs (issue #565), plugin part: the Apps menu with plugin items, the
// media-area stage with its tabs, a plugin side panel, a plugin web dialog, the security score of
// an unofficial plugin (Browse card and the details breakdown) and Settings → Plugins, each in
// dark and light. One client (Alex) on the mumpeer test environment's server with the fixture plugins of
// scripts/e2e-plugin-registry.sh (MUMCORD_E2E_PLUGIN_PACKAGES); the plugin names are the test
// fixtures' own, the bots' names are made up.
//
// The client runs on a display of its own at 2x scale, like the camera part; the pictures are
// scaled to 1440×900 afterwards.
//
// Not part of the suite (outside specs/). Run:
//   npm run e2e -- --spec e2e/walkthrough/screenshots-plugins.e2e.ts
import type { ChildProcess } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, browser } from "@wdio/globals";
import { app, apps, pluginenv, relayenv } from "../helpers/index.ts";
import { botInBackground, type BackgroundBot } from "../helpers/bot.ts";
import { sendText } from "../helpers/chat-ui.ts";
import * as files from "../helpers/files-ui.ts";
import { addAndConnect, completeWizard, joinIfReadOnly } from "../helpers/voice-ui.ts";
import {
  card,
  closeSettings,
  openPlugins,
  waitForCard,
  type PluginsTab,
} from "../helpers/score-ui.ts";
import { openSettingsGroup } from "../helpers/settings-ui.ts";
import {
  HIDPI_ENV,
  both,
  closeOverlays,
  dropOnA,
  moduleBot,
  openAppsMenu,
  scrollPage,
  sleep,
  startHidpiDisplay,
} from "./shots.ts";

const BOTS = ["Priya", "Ben", "Zoe"];
const SHARED_FILE = "raid-notes.pdf";
const SHARED_CLIP = "boss-fight-clip.mp4";
const WHITEBOARD = "Whiteboard";
const SKYLOOM = "Skyloom";
/** Every optional permission, so the chat cards, side panels and chat lines are on. */
const OPTIONAL: Record<string, string[]> = {
  whiteboard: [
    "ui.side-panel",
    "events.voice",
    "chat.send",
    "chat.supersedes",
    "ui.chat-card",
    "commands",
    "files.save",
    "chat.post-image",
  ],
  skyloom: [
    "peers.relay",
    "audio.effects",
    "events.voice",
    "chat.send",
    "chat.supersedes",
    "ui.chat-card",
    "ui.member-badge",
    "commands",
    "notify",
  ],
};
const SCORED = "Fixture score-high";

async function install(id: string): Promise<void> {
  const optional = OPTIONAL[id] ?? [];
  await app.invoke("plugins_index", { language: "en" });
  await app.invoke("plugin_install_prepare", { id });
  await app.invoke("plugin_install", {
    id,
    version: null,
    optional,
    visibility: "everyone",
    confirmed: true,
  });
  await browser.waitUntil(
    async () => {
      const list = await app.invoke<{ plugins: { id: string; status: { kind: string } }[] }>(
        "plugins_list",
      );
      return list.plugins.find((p) => p.id === id)?.status.kind === "running";
    },
    { timeout: 30_000, interval: 250, timeoutMsg: `${id} never started` },
  );
}

function appItem(label: string) {
  return $(
    `//div[contains(@class,'chat-apps__menu')]//button[contains(@class,'appmenu__item')][.//span[contains(@class,'appmenu__text')]/text()[normalize-space()='${label}']]`,
  );
}

async function chooseApp(label: string): Promise<void> {
  await openAppsMenu();
  const item = appItem(label);
  await item.waitForClickable({ timeout: 20_000 });
  await item.click();
  await $("div.chat-apps__menu").waitForExist({ reverse: true, timeout: 5_000 });
}

const frame = (plugin: string) =>
  $(`iframe.plugin-webview__frame[title='${plugin}, plugin content']`);
const sidePanelFrame = () => $(".pstrip-panel iframe.plugin-webview__frame");

/** Runs `fn` inside a plugin's page, then returns to Mumcord. */
async function inFrame<T>(el: ReturnType<typeof $>, fn: () => Promise<T>): Promise<T> {
  await el.waitForExist({ timeout: 30_000 });
  await browser.switchFrame(el);
  try {
    return await fn();
  } finally {
    await browser.switchFrame(null);
  }
}

/** Whiteboard: picks a tool and drags along `points` (pixels from the board's centre). */
async function draw(toolId: string, points: [number, number][]): Promise<void> {
  const button = $(`#toolbar button[data-tool='${toolId}']`);
  await button.waitForClickable({ timeout: 15_000 });
  await button.click();
  const overlay = $("#overlay");
  await overlay.waitForDisplayed({ timeout: 15_000 });
  const [first, ...rest] = points;
  const pointer = browser.action("pointer", { parameters: { pointerType: "mouse" } });
  pointer.move({ x: first[0], y: first[1], origin: overlay }).down({ button: 0 });
  for (const [x, y] of rest) pointer.move({ x, y, origin: overlay, duration: 30 });
  await pointer.up({ button: 0 }).perform();
}

async function stickyNote(x: number, y: number, text: string): Promise<void> {
  await draw("note", [[x, y]]);
  await $("#editor").waitForDisplayed({ timeout: 10_000 });
  await browser.keys(text);
  await browser.keys("Escape");
  await $("#editor").waitForDisplayed({ reverse: true, timeout: 10_000 });
}

/** Clicks Join on the newest chat card or stage activity of a plugin. */
async function joinPlugin(xpath: string): Promise<void> {
  const join = $(xpath);
  await join.waitForClickable({ timeout: 30_000 });
  await join.click();
}

async function openSidePanel(plugin: string): Promise<void> {
  const icon = $(`.pstrip-icons button[aria-label='${plugin}']`);
  await icon.waitForClickable({ timeout: 30_000 });
  if ((await icon.getAttribute("aria-pressed")) !== "true") await icon.click();
  await $(".pstrip-panel").waitForDisplayed({ timeout: 15_000 });
}

/** The fixture repositories are plain http on localhost, which only Developer mode accepts. */
async function developerMode(): Promise<void> {
  await openSettingsGroup("Advanced & Developer");
  const developer = $("//input[@role='switch'][@id=//label[.='Developer mode']/@for]");
  await developer.waitForExist({ timeout: 10_000 });
  // A local http registry is only allowed in Developer mode, so make sure it ends up on.
  if (!(await developer.isSelected())) await developer.click();
  await browser.waitUntil(() => developer.isSelected(), {
    timeout: 10_000,
    timeoutMsg: "Developer mode did not switch on",
  });
  await browser.keys("Escape");
  await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
}

const pluginsPage = (tab: PluginsTab, fraction: number) => async () => {
  await openPlugins(tab);
  await sleep(1_000);
  await scrollPage(fraction);
};

async function openScoreDetails(): Promise<void> {
  await openPlugins("Browse");
  await waitForCard(SCORED);
  await card(SCORED).$(`button[aria-label='Details of ${SCORED}']`).click();
  await $("section.pd-score").waitForDisplayed({ timeout: 30_000 });
}

(relayenv.relayAvailable() ? describe : describe.skip)("0.7.0 screenshots: plugins", function () {
  this.timeout(30 * 60_000);
  this.bail(true);

  let xvfb: ChildProcess | undefined;
  const bots: BackgroundBot[] = [];
  let relayPort = 0;
  const moduleBots: ChildProcess[] = [];

  before(async function () {
    this.timeout(20 * 60_000);
    xvfb = await startHidpiDisplay();
    const fixtures = await pluginenv.startPluginFixtures({
      official: {},
      thirdParty: {},
      https: true,
    });
    // The relay environment, so plugin data, camera and files show as available.
    const { port } = await relayenv.startRelay("all");
    relayPort = port;
    await app.restart({ freshProfile: true, env: pluginenv.appEnv(HIDPI_ENV) });
    await browser.setWindowSize(1440, 900);
    await completeWizard("Alex");
    await addAndConnect(`localhost:${String(port)}`);
    await joinIfReadOnly();
    await files.registerOnServer();
    await developerMode();
    await app.invoke("plugin_repo_add", { url: fixtures.thirdPartyUrl, expectedKey: null });
    for (const id of ["whiteboard", "skyloom"]) await install(id);

    await apps.startB({ env: pluginenv.appEnv() });
    await apps.onB(async () => {
      await completeWizard("Maya");
      await developerMode();
      await addAndConnect(`localhost:${String(port)}`);
      await joinIfReadOnly();
      await files.registerOnServer();
      for (const id of ["whiteboard", "skyloom"]) await install(id);
    });

    for (const name of BOTS) {
      const bot = botInBackground(["script"], { name, port, timeout: 1_800 });
      bot.send("sleep 1700");
      bots.push(bot);
    }
    moduleBots.push(moduleBot("DJ Nova", port, ["music", "history"], port + 50));
    moduleBots.push(moduleBot("Chronicle", port, ["history"], port + 51));
    await sendText("Who's up for a round of something after the raid?");
    await sleep(3_000);
  });

  after(async () => {
    for (const bot of bots) bot.stop();
    for (const child of moduleBots) child.kill();
    await apps.stopB();
    await pluginenv.stopPluginFixtures();
    await relayenv.stopRelay();
    xvfb?.kill();
  });

  it("bots: the Bot label in the channel tree and Bots on this server", async () => {
    await $("//*[contains(@class,'bots-section')][contains(.,'(3)')]").waitForDisplayed({
      timeout: 60_000,
    });
    // The section starts collapsed; the shot shows the bots in it.
    const toggle = $(".bots-section .collapsible-section__toggle");
    if ((await toggle.getAttribute("aria-expanded")) === "false") await toggle.click();
    await both("bots-on-server");
  });

  it("file sharing: the chip, the sender's card, the received card and its download", async () => {
    const dir = mkdtempSync(join(tmpdir(), "mumcord-shots-"));
    const notes = files.randomFile(join(dir, "a"), SHARED_FILE, 240_000);
    const clip = files.sparseFile(
      mkdtempSync(join(tmpdir(), "mumcord-shots-")),
      SHARED_CLIP,
      95_000_000,
    );

    await files.waitSharingReady();
    await dropOnA([notes]);
    await files.waitChipReady(SHARED_FILE);
    await both("file-share-chip", () => Promise.resolve(), true);
    await files.send();
    await apps.onB(async () => {
      await files.waitSharingReady();
      await files.card(SHARED_FILE).waitForDisplayed({ timeout: 60_000 });
      await files.download(SHARED_FILE);
      await files.waitDownloaded(SHARED_FILE);
      await sendText("Got the notes, thanks! Here's the clip from last night's boss fight.");
      await files.dropFiles("b", [clip]);
      await files.waitChipReady(SHARED_CLIP);
      await files.send();
    });
    await files.card(SHARED_CLIP).waitForDisplayed({ timeout: 60_000 });
    await sendText("Nice, downloading it now.");
    await both(
      "file-share-own-card",
      async () => {
        await files.card(SHARED_FILE).scrollIntoView({ block: "center" });
      },
      true,
    );
    await both(
      "file-share-card",
      async () => {
        await files.card(SHARED_CLIP).scrollIntoView({ block: "center" });
      },
      true,
    );
    await both("file-share-download-dialog", async () => {
      await files.cardButton(SHARED_CLIP, "Download").click();
      await files.dialog().waitForDisplayed({ timeout: 10_000 });
    });
    await closeOverlays();
    await files.download(SHARED_CLIP);
    await files.waitCardText(SHARED_CLIP, "Downloading", 30_000).catch(() => undefined);
    await both(
      "file-share-downloading",
      async () => {
        await files.card(SHARED_CLIP).scrollIntoView({ block: "center" });
      },
      true,
    );
    await files.waitDownloaded(SHARED_CLIP, 300_000);
    await both(
      "file-share-downloaded",
      async () => {
        await files.card(SHARED_CLIP).scrollIntoView({ block: "center" });
      },
      true,
    );
  });

  it("Apps menu with plugin items", async () => {
    await both("apps-menu-plugins", openAppsMenu);
    await closeOverlays();
  });

  it("Whiteboard over the relay with drawings from Alex and Maya", async () => {
    await chooseApp("Start whiteboard");
    await frame(WHITEBOARD).waitForDisplayed({ timeout: 30_000 });
    await apps.onB(async () => {
      await joinPlugin(
        `(//section[contains(@class,'plugin-chat-card')][@aria-label='Card from ${WHITEBOARD}']//button[normalize-space(.)='Join'])[last()]`,
      );
      await frame(WHITEBOARD).waitForDisplayed({ timeout: 30_000 });
    });
    await inFrame(frame(WHITEBOARD), async () => {
      // A small map of the raid route: the gate, the bridge and the path between them.
      await draw("pen", [
        [-260, 60],
        [-260, -20],
        [-200, -60],
        [-140, -20],
        [-140, 60],
        [-260, 60],
      ]);
      await draw("pen", [
        [-140, 20],
        [-80, 10],
        [-20, 30],
        [40, 10],
        [100, 20],
      ]);
      await stickyNote(-200, 120, "Gate: meet here at 9");
    });
    // Maya's own strokes wait for the Whiteboard plugin's second-user loading fix (plugins #20).
    await sleep(4_000);
    await both("plugin-whiteboard", () => Promise.resolve(), true);
  });

  it("Skyloom running with two players, next to the Whiteboard tab", async () => {
    await chooseApp("Start Skyloom");
    await apps.onB(async () => {
      // Maya joins from the "Alex started Skyloom · Join" notice above the stage.
      await joinPlugin(
        "//*[contains(normalize-space(.),'started Skyloom')]/descendant-or-self::*[(self::button or self::a) and normalize-space(.)='Join'][1]",
      );
      // Joining adds a tab; the stage keeps showing the Whiteboard until Skyloom's tab is picked.
      const tab = $(
        "//button[@role='tab'][contains(@class,'media-stage__tab')][.//*[contains(@class,'media-stage__label')][normalize-space()='Skyloom']]",
      );
      // With a single activity there is no tab row; the stage shows Skyloom directly.
      await sleep(3_000);
      if (await tab.isExisting()) await tab.click();
      await frame(SKYLOOM).waitForDisplayed({ timeout: 30_000 });
    });
    // Alex's stage shows Skyloom too.
    const ownTab = $(
      "//button[@role='tab'][contains(@class,'media-stage__tab')][.//*[contains(@class,'media-stage__label')][normalize-space()='Skyloom']]",
    );
    if (await ownTab.isExisting()) await ownTab.click();
    await frame(SKYLOOM).waitForDisplayed({ timeout: 30_000 });
    await inFrame(frame(SKYLOOM), async () => {
      await $("button[data-intent='weave'][data-value='60']").click();
      const begin = $("button[data-intent='begin']");
      await begin.waitForEnabled({ timeout: 15_000 });
      await begin.click();
      await browser.waitUntil(async () => (await $("body").getText()).includes("dawn in"), {
        timeout: 30_000,
      });
    });
    await sleep(3_000);
    await both("plugin-stage-tabs", () => Promise.resolve(), true);
  });

  it("activity log in the members column", async () => {
    // A late joiner adds a "connected" line next to the plugin activities.
    const leo = botInBackground(["script"], { name: "Leo", port: relayPort, timeout: 600 });
    leo.send("sleep 590");
    bots.push(leo);
    await sleep(4_000);
    const tab = $(
      "//div[contains(@class,'activity-tabs__row')]//button[@role='tab'][normalize-space()='Activity']",
    );
    await tab.waitForClickable({ timeout: 10_000 });
    await tab.click();
    await both("activity-log-members", () => Promise.resolve(), true);
    await $(
      "//div[contains(@class,'activity-tabs__row')]//button[@role='tab'][normalize-space()='Members']",
    ).click();
  });

  it.skip("Whiteboard side panel and its Board settings dialog", async () => {
    await openSidePanel(WHITEBOARD);
    await sidePanelFrame().waitForDisplayed({ timeout: 20_000 });
    await sleep(3_000);
    await both("plugin-side-panel", () => Promise.resolve(), true);
    await $(
      `//button[@role='tab'][contains(@class,'media-stage__tab')][.//*[contains(@class,'media-stage__label')][normalize-space()='${WHITEBOARD}']]`,
    ).click();
    await both("plugin-web-dialog", async () => {
      await inFrame(frame(WHITEBOARD), () => $("button[aria-label='Board settings']").click());
      await $("//div[@role='dialog'][.//*[normalize-space(.)='Board settings']]").waitForDisplayed({
        timeout: 15_000,
      });
    });
    await closeOverlays();
  });

  it("security score: Browse card and the details breakdown", async () => {
    await both("plugin-score-browse", async () => {
      await openPlugins("Browse");
      await waitForCard(SCORED);
      await card(SCORED).scrollIntoView({ block: "center" });
    });
    await both("plugin-score-details", openScoreDetails);
    await both("plugin-score-details-2", async () => {
      await openScoreDetails();
      await $("section.pd-score").scrollIntoView({ block: "start" });
    });
    await closeSettings();
  });

  it("Settings → Plugins: Installed with Whiteboard and Skyloom, and Updates", async () => {
    await both("settings-plugins-installed-1", pluginsPage("Installed", 0));
    await both("settings-plugins-installed-3", pluginsPage("Installed", 1));
    await both("settings-plugins-updates", async () => {
      await pluginsPage("Updates", 0)();
      await $(
        "//summary[normalize-space()='Update settings'] | //button[normalize-space()='Update settings']",
      ).click();
    });
    await closeSettings();
  });

  it("Settings → Plugins: Browse and Repositories", async () => {
    await both("settings-plugins-browse", pluginsPage("Browse", 0));
    await both("settings-plugins-repositories", pluginsPage("Repositories", 0));
    await closeSettings();
  });
});
