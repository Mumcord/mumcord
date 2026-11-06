// Spec 09 (plugin platform 1.1), the media-area stage and its activities with three Mumcord users
// in one channel of the local test server:
//
//   AC-9.11  A watches B's screen share and starts Test Stage from the Apps menu: a tab with a dot,
//            the share stays shown, switching (click, Alt+1/Alt+2) stops nothing; B (with the
//            plugin) sees "Alice started Test Stage · Join", C (without it) sees nothing.
//   AC-9.12  Test Stage Official (official fixture registry): C sees "Alice is playing … · Get
//            plugin", which opens the install prompt.
//   AC-9.13  a start past the 8 s gesture window gets not-permitted, an Apps-menu click works.
//   AC-9.14  a second activity of one plugin and a fourth in the channel show the limit text; a
//            third participant (max_participants 2) gets "This activity is full".
//   AC-9.15  full screen (banner, Esc), pop-out window (resize, pop in), mini bar, leave, the
//            activity ending when the last one leaves, and A moving to another channel leaving it
//            with the plugin getting stage-left.
//
// Client A = Alice (starter), B = Bob (has the stage plugins), C = Carol (no plugin at first).
// The fixtures come from scripts/e2e-plugin-registry.sh (MUMCORD_E2E_PLUGIN_PACKAGES): test-stage,
// test-surface and test-relay from the third-party repository, test-stage-official from the
// official fixture registry.
//
// Manual (not automatable on the Linux runner):
//   - AC-9.13: a genuine click or key press inside the plugin's own web page counts as a gesture,
//     a click the page fakes by script does not. No fixture page asks its plugin for a
//     gesture-bound action yet, and WebKitWebDriver cannot tell a trusted from a scripted click
//     in a plugin frame.
//   - AC-9.15: the pop-out's always-on-top and resizing it with the mouse on Windows and macOS.
import { $, $$, browser, expect } from "@wdio/globals";
import { app, apps, pluginenv, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  finishWizard,
  joinChannel,
  sleep,
  startScreenShare,
} from "../../helpers/chat-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";

const STAGE = "Test Stage";
const OFFICIAL = "Test Stage Official";
const SURFACE = "Test Surface";
const TOO_MANY = "Too many activities in this channel";

interface Activity {
  id: string;
  plugin: string;
  joined: boolean;
  participants: number;
  kind: "activity" | "getPlugin";
}

/**
 * Calls a backend command; a rejection comes back as its text instead of a thrown error (the
 * WebKit driver turns a rejected command into a WebDriver error even when the page catches it).
 */
async function call(command: string, args: object = {}): Promise<{ ok: unknown; error?: string }> {
  try {
    return { ok: await app.invoke(command, args) };
  } catch (e) {
    return { ok: null, error: e instanceof Error ? e.message : String(e) };
  }
}

async function invokeOk(command: string, args: object = {}): Promise<unknown> {
  const result = await call(command, args);
  if (result.error !== undefined) throw new Error(`${command} failed: ${result.error}`);
  return result.ok;
}

async function activities(): Promise<Activity[]> {
  return (await invokeOk("stage_activities")) as Activity[];
}

/** Installs a store plugin as the prompt would (prepare, then install) and waits until it runs. */
async function install(id: string): Promise<void> {
  await invokeOk("plugins_index", { language: "en" });
  await invokeOk("plugin_install_prepare", { id });
  await invokeOk("plugin_install", {
    id,
    version: null,
    optional: [],
    visibility: "everyone",
    confirmed: true,
  });
  await browser.waitUntil(
    async () => {
      const list = (await invokeOk("plugins_list")) as {
        plugins: { id: string; status: { kind: string } }[];
      };
      return list.plugins.find((p) => p.id === id)?.status.kind === "running";
    },
    { timeout: 30_000, interval: 250, timeoutMsg: `${id} never started` },
  );
}

/**
 * Wizard, server, voice, Developer mode (the fixture repository is plain http on localhost, which
 * only Developer mode accepts) and the third-party repository.
 */
async function setUp(name: string, repoUrl: string): Promise<void> {
  await finishWizard(name);
  await addFirstServer(testenv.ports.v15);
  // A fresh connect is read-only (AC-1.35); join to be in the channel.
  const join = $(".userpanel-readonly__join");
  await join.waitForClickable({ timeout: 15_000 });
  await join.click();
  await $(".userpanel-readonly").waitForExist({ reverse: true, timeout: 15_000 });
  await openSettingsGroup("Advanced & Developer");
  const developer = $("//input[@role='switch'][@id=//label[.='Developer mode']/@for]");
  await developer.waitForExist({ timeout: 10_000 });
  await developer.click();
  await expect(developer).toBeSelected();
  // Watching B's screen share is the first direct connection; deciding here keeps the one-time
  // "Allow direct connections?" prompt (FR-2.36) from covering the window mid-test.
  await openSettingsGroup("Privacy & Connections");
  const direct = $("//input[@role='switch'][@id=//label[.='Allow direct connections to other Mumcord users']/@for]");
  await direct.waitForExist({ timeout: 10_000 });
  await direct.click();
  await expect(direct).toBeSelected();
  await browser.keys("Escape");
  await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
  await browser.waitUntil(
    async () => (await call("plugin_repo_add", { url: repoUrl, expectedKey: null })).error === undefined,
    { timeout: 10_000, timeoutMsg: "the fixture repository could not be added" },
  );
}

/** A plugin's item in the composer's Apps menu, by its exact label. */
function appItem(label: string) {
  return $(
    `//div[contains(@class,'chat-apps__menu')]//button[contains(@class,'appmenu__item')][.//span[contains(@class,'appmenu__text')]/text()[normalize-space()='${label}']]`,
  );
}

/** Opens the Apps menu and chooses a plugin item (the click is the plugin's gesture). */
async function chooseApp(label: string): Promise<void> {
  const button = $("button.chat-apps__button");
  await button.waitForClickable({ timeout: 20_000 });
  await button.click();
  const item = appItem(label);
  await item.waitForClickable({ timeout: 20_000, timeoutMsg: `the Apps menu has no "${label}"` });
  await item.click();
  await $("div.chat-apps__menu").waitForExist({ reverse: true, timeout: 5_000 });
}

/** Opens a plugin's panel in the strip (no-op when it is open). */
async function openPanel(plugin: string): Promise<void> {
  const icon = $(`button.pstrip-icon[aria-label='${plugin}']`);
  await icon.waitForExist({ timeout: 20_000, timeoutMsg: `${plugin} has no strip icon` });
  // An open panel is left alone: reading its log must work while a plugin dialog covers it.
  if ((await icon.getAttribute("aria-pressed")) !== "true") {
    await icon.waitForClickable({ timeout: 10_000 });
    await icon.click();
  }
}

/**
 * The lines of the open plugin panel's event log (its text rows). Read from the DOM, because the log sits at the
 * bottom of the panel's scroll area and WebDriver's visible text leaves out what is scrolled away.
 */
async function panelLog(): Promise<string> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll(".pstrip-body li .pr-text"))
      .map((line) => line.textContent.trim())
      .join("\n"),
  );
}

/** Waits until the plugin's panel (its event log) shows a line containing `text`. */
async function waitLog(plugin: string, text: string, timeout = 20_000): Promise<void> {
  await openPanel(plugin);
  let seen = "";
  // timeoutMsg is fixed when the wait starts, so the panel text seen last is reported by hand.
  await browser
    .waitUntil(
      async () => {
        seen = await panelLog();
        return seen.includes(text);
      },
      { timeout, interval: 250 },
    )
    .catch(() => {
      throw new Error(`${plugin} panel never showed "${text}"; it shows:\n${seen}`);
    });
}

/** The activity line of a plugin in the channel ("X started P · Join" / "X is playing P"). */
function activityLine(plugin: string) {
  // "Test Stage" is a prefix of "Test Stage Official", which peers now see as well.
  const notLonger = plugin === STAGE ? `[not(contains(normalize-space(),'${OFFICIAL}'))]` : "";
  return $(
    `//li[contains(@class,'plugin-activity')][.//span[contains(@class,'plugin-activity__text')][contains(normalize-space(),'${plugin}')]${notLonger}]`,
  );
}

async function linesText(): Promise<string> {
  const list = $("ul.plugin-activities");
  return (await list.isExisting()) ? list.getText() : "";
}

function stageTab(label: string) {
  return $(`//button[@role='tab'][contains(@class,'media-stage__tab')][.//*[contains(@class,'media-stage__label')][normalize-space()='${label}']]`);
}

async function selectedTab(): Promise<string> {
  const tab = $("//button[@role='tab'][contains(@class,'media-stage__tab')][@aria-selected='true']");
  await tab.waitForExist({ timeout: 10_000 });
  return (await tab.getAttribute("aria-label")) ?? "";
}

async function expectToast(text: string): Promise<void> {
  await browser.waitUntil(
    async () => {
      for (const toast of await $$(".plugin-toast")) {
        if ((await toast.getText()).includes(text)) return true;
      }
      return false;
    },
    { timeout: 10_000, interval: 200, timeoutMsg: `no toast "${text}"` },
  );
}

/** The stage frame of a plugin in the media area. */
function stageFrame(plugin: string) {
  return $(`.plugin-stage[aria-label='Media area: ${plugin}']`);
}

function stageButton(plugin: string, text: string) {
  return $(
    `//*[contains(@class,'plugin-stage__bar')][@aria-label='${plugin} controls']//button[normalize-space()='${text}' or @aria-label='${text}']`,
  );
}

describe("plugins 1.1: stage and activities (AC-9.11 … AC-9.15)", function () {
  this.timeout(30 * 60_000);
  let fixtures: pluginenv.PluginFixtures;

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await pluginenv.startPluginFixtures({ official: {}, thirdParty: {}, https: true });
    const repoUrl = fixtures.thirdPartyUrl ?? "";
    await testenv.up("v15");

    await app.restart({ env: pluginenv.appEnv() });
    await setUp("Alice", repoUrl);
    for (const id of ["test-stage", "test-stage-official", "test-surface", "test-relay"]) {
      await install(id);
    }

    await apps.startB({ env: pluginenv.appEnv() });
    await apps.onB(async () => {
      await setUp("Bob", repoUrl);
      await install("test-stage");
    });

    await apps.startC({ env: pluginenv.appEnv() });
    await apps.onC(() => setUp("Carol", repoUrl));
  });

  after(async () => {
    await apps.stopC();
    await apps.stopB();
    await pluginenv.stopPluginFixtures();
  });

  it("AC-9.13: a start after the 8 s gesture window is refused", async () => {
    await chooseApp("Start Test Stage later");
    await waitLog(STAGE, "app-menu: late");
    // The plugin starts 10 s after the click, past the 8 s window.
    await waitLog(STAGE, "late-start: not-permitted", 30_000);
    expect((await activities()).some((a) => a.plugin === "test-stage")).toBe(false);
  });

  it("AC-9.11: a new activity is a dot tab next to B's screen share and never takes the stage", async () => {
    await apps.onB(startScreenShare);
    const watch = $(
      "//div[contains(@class,'activity-banner')][contains(.,'is sharing their screen')]//button[normalize-space()='Watch']",
    );
    await watch.waitForClickable({ timeout: 30_000, timeoutMsg: "A never saw B's share" });
    await watch.click();
    const share = $(".media-frame:not(.plugin-stage)");
    await share.waitForDisplayed({ timeout: 30_000, timeoutMsg: "B's share is not shown" });

    // AC-9.13: within 8 s of the Apps-menu click the start works.
    await chooseApp("Start Test Stage");
    await waitLog(STAGE, "start: ok");
    const tab = stageTab(STAGE);
    await tab.waitForDisplayed({ timeout: 15_000, timeoutMsg: "no Test Stage tab" });
    await expect(tab.$(".media-stage__dot")).toBeExisting();
    expect(await tab.getAttribute("aria-selected")).not.toBe("true");
    expect(await share.isDisplayed()).toBe(true);

    await tab.click();
    await expect(tab).toHaveAttribute("aria-selected", "true");
    await stageFrame(STAGE).waitForDisplayed({ timeout: 10_000 });
    await expect(tab.$(".media-stage__dot")).not.toBeExisting();

    await browser.keys(["Alt", "1"]);
    await browser.waitUntil(async () => !(await selectedTab()).startsWith(STAGE), {
      timeout: 5_000,
      timeoutMsg: "Alt+1 did not select the share",
    });
    await share.waitForDisplayed({ timeout: 10_000 });
    await browser.keys(["Alt", "2"]);
    await browser.waitUntil(async () => (await selectedTab()).startsWith(STAGE), {
      timeout: 5_000,
      timeoutMsg: "Alt+2 did not select Test Stage",
    });
    // Switching stops neither: both stay listed and running.
    expect((await activities()).find((a) => a.plugin === "test-stage")?.joined).toBe(true);
    await apps.onB(async () => {
      await expect($("//div[contains(@class,'sharer-bar')]")).toBeExisting();
    });

    await apps.onB(async () => {
      await activityLine(STAGE).waitForDisplayed({ timeout: 20_000 });
      await expect(activityLine(STAGE)).toHaveText(
        expect.stringContaining(`Alice started ${STAGE}`),
      );
      await expect(activityLine(STAGE).$("button.plugin-activity__action")).toHaveText("Join");
      await $(".sharer-bar button[aria-label='Stop sharing']").click();
    });
    await apps.onC(async () => {
      // B already sees it, so the announcement has reached the channel; C has no such plugin and
      // this repository is not official, so nothing is shown.
      await sleep(2_000);
      expect(await linesText()).not.toContain(STAGE);
    });
  });

  it("AC-9.12: C sees Get plugin for an official plugin it lacks; it opens the install prompt", async () => {
    await chooseApp("Start Test Stage Official");
    await waitLog(OFFICIAL, "start: ok");
    await apps.onC(async () => {
      const line = activityLine(OFFICIAL);
      await line.waitForDisplayed({ timeout: 20_000, timeoutMsg: "C has no line for the official plugin" });
      await expect(line).toHaveText(expect.stringContaining(`Alice is playing ${OFFICIAL}`));
      const get = line.$(`button[aria-label='Get ${OFFICIAL}']`);
      await expect(get).toHaveText("Get plugin");
      await get.click();
      const prompt = $(".plugin-prompt");
      await prompt.waitForDisplayed({ timeout: 30_000, timeoutMsg: "Get plugin opened no install prompt" });
      await expect(prompt).toHaveText(expect.stringContaining(OFFICIAL));
      await browser.keys("Escape");
      await prompt.waitForExist({ reverse: true, timeout: 10_000 });
    });
  });

  it("AC-9.14: one activity per plugin, three per channel, and a full activity", async () => {
    await chooseApp("Start Test Stage");
    await waitLog(STAGE, "start: quota-exceeded");
    await expectToast(TOO_MANY);

    await chooseApp("Start Test Surface");
    await waitLog(SURFACE, "start-stage: ok");
    await chooseApp("Start Test Relay");
    await expectToast(TOO_MANY);
    expect((await activities()).filter((a) => a.kind === "activity")).toHaveLength(3);

    // B joins through the activity line: the second of two places.
    await apps.onB(async () => {
      const join = activityLine(STAGE).$(`button[aria-label='Join ${STAGE}']`);
      await join.waitForClickable({ timeout: 10_000 });
      await join.click();
      // B shares its own screen and watches nothing else, so the activity is B's only item: the
      // tab row stays hidden (FR-9.44) and the stage shows the activity directly.
      await stageFrame(STAGE).waitForDisplayed({ timeout: 15_000 });
      await expect(activityLine(STAGE).$("button.plugin-activity__action")).toHaveText("Open");
    });
    await waitLog(STAGE, "joined: Bob");

    // C gets the plugin now and is the third: max_participants is 2.
    await apps.onC(async () => {
      await install("test-stage");
      const join = activityLine(STAGE).$(`button[aria-label='Join ${STAGE}']`);
      await join.waitForClickable({ timeout: 20_000 });
      await join.click();
      await expect(activityLine(STAGE).$(".plugin-activity__error")).toHaveText(
        "This activity is full",
        { wait: 15_000 },
      );
    });
  });

  it("AC-9.15: full screen, pop-out and mini bar", async () => {
    await stageTab(STAGE).click();
    const frame = stageFrame(STAGE);
    await frame.waitForDisplayed({ timeout: 10_000 });

    const full = stageButton(STAGE, "Full screen");
    await full.click();
    const banner = $("p.plugin-stage__banner");
    await expect(banner).toHaveText(`${STAGE} is full screen · Esc to exit`, { wait: 5_000 });
    // The banner goes after 3 s; full screen stays until Esc.
    await banner.waitForExist({ reverse: true, timeout: 8_000 });
    await expect(stageButton(STAGE, "Leave full screen")).toHaveAttribute("aria-pressed", "true");
    await browser.keys("Escape");
    await expect(stageButton(STAGE, "Full screen")).toHaveAttribute("aria-pressed", "false", {
      wait: 5_000,
    });

    const main = await browser.getWindowHandle();
    await stageButton(STAGE, "Pop out").click();
    let popout = "";
    await browser.waitUntil(
      async () => {
        popout = (await browser.getWindowHandles()).find((h) => h !== main) ?? "";
        return popout !== "";
      },
      { timeout: 15_000, timeoutMsg: "no pop-out window" },
    );
    await browser.switchToWindow(popout);
    try {
      await stageFrame(STAGE).waitForDisplayed({ timeout: 15_000 });
      await browser.setWindowSize(700, 520);
      const size = await browser.getWindowSize();
      expect(Math.abs(size.width - 700)).toBeLessThan(40);
      await expect(stageFrame(STAGE)).toBeDisplayed();
    } finally {
      await browser.switchToWindow(main);
    }

    // While it is popped out the main window keeps a mini bar to come back.
    const mini = $(`div.mini-bar[aria-label='Media area: ${STAGE}']`);
    if (await mini.isExisting()) await expect(mini).toBeDisplayed();
    await browser.switchToWindow(popout);
    try {
      await browser.keys(["Control", "w"]);
    } catch (error) {
      // The window closes on the key press, so releasing the keys finds no window; the wait
      // below checks that it really closed.
      if (!/no such window/i.test(String(error))) throw error;
    } finally {
      await browser.switchToWindow(main);
    }
    await browser.waitUntil(async () => (await browser.getWindowHandles()).length === 1, {
      timeout: 10_000,
      timeoutMsg: "the pop-out did not close with Ctrl+W",
    });
    await frame.waitForDisplayed({ timeout: 10_000 });
  });

  it("AC-9.15: leaving, ending when empty, and moving to another channel", async () => {
    await apps.onB(async () => {
      await stageFrame(STAGE).waitForDisplayed({ timeout: 10_000 });
      await stageButton(STAGE, `Leave ${STAGE}`).click();
      await expect(activityLine(STAGE).$("button.plugin-activity__action")).toHaveText("Join", {
        wait: 15_000,
      });
    });
    await waitLog(STAGE, "left: Bob");

    await stageTab(STAGE).click();
    await stageButton(STAGE, `Leave ${STAGE}`).click();
    await apps.onB(async () => {
      await activityLine(STAGE).waitForExist({ reverse: true, timeout: 20_000 });
    });
    expect((await activities()).some((a) => a.plugin === "test-stage")).toBe(false);

    // Test Stage Official is still running; moving away leaves it and the plugin hears it.
    await joinChannel("Raid");
    await waitLog(OFFICIAL, "left: Alice");
    expect((await activities()).some((a) => a.plugin === "test-stage-official" && a.joined)).toBe(
      false,
    );
    await apps.onC(async () => {
      await activityLine(OFFICIAL).waitForExist({ reverse: true, timeout: 20_000 });
    });
  });
});
