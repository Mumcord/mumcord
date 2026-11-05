// A trusted click inside a plugin's own web page is a user gesture (spec 09, FR-9.45, AC-9.13).
//
// Test Surface (third-party fixture repository) opens its side panel from a panel button. Its
// page has "Start stage from page", which asks the plugin to call `stage.start`, a gesture-bound
// call. After the panel click's 8 s window has run out:
// - a click the page makes by script (and a gesture report with a guessed key) does not count:
//   the plugin logs `side-start-stage: not-permitted`;
// - a real click on the same button counts: `side-start-stage: ok`.
import { $, browser, expect } from "@wdio/globals";
import { app, pluginenv, testenv } from "../../helpers/index.ts";
import { addFirstServer, finishWizard } from "../../helpers/chat-ui.ts";
import { fill } from "../../helpers/servers-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { confirmInstall, openPrompt } from "../../helpers/score-ui.ts";

const NAME = "Test Surface";
/** FR-9.45's gesture window. */
const GESTURE_WINDOW_MS = 8_000;

const pluginTab = (name: string) =>
  $(`//button[@role='tab'][contains(@class,'plugins-tab')][starts-with(normalize-space(.),'${name}')]`);
const card = (name: string) =>
  $(`//li[@class='plugin-card'][.//*[contains(@class,'plugin-card-name')][normalize-space(.)='${name}']]`);
const panelLine = (text: string) =>
  $(`//*[contains(@class,'pstrip-panel')]//*[normalize-space(.)='${text}']`);
const sideFrame = () =>
  $(`section.plugin-sidepanel[aria-label='${NAME} side panel'] iframe.plugin-webview__frame`);

async function closeSettings(): Promise<void> {
  await browser.keys("Escape");
  await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
}

/** Runs `inside` with the side panel's page as the current frame. */
async function inSidePage(inside: () => Promise<void>): Promise<void> {
  await browser.switchFrame(await sideFrame());
  try {
    await inside();
  } finally {
    await browser.switchFrame(null);
  }
}

describe("plugins: a trusted click in the plugin's page is a gesture (FR-9.45, AC-9.13)", () => {
  let fixtures: pluginenv.PluginFixtures;

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await pluginenv.startPluginFixtures({ thirdParty: {} });
    await testenv.up("v15");
    await app.restart({ env: pluginenv.appEnv() });
    await finishWizard();
    await addFirstServer(testenv.ports.v15);
    // The fixture repository serves plain http on 127.0.0.1, which the client only accepts in
    // Developer mode (FR-6.74).
    await openSettingsGroup("Advanced & Developer");
    const developerMode = $("//input[@role='switch'][@id=//label[.='Developer mode']/@for]");
    await developerMode.waitForExist({ timeout: 10_000 });
    await developerMode.click();
    await expect(developerMode).toBeSelected();

    await openSettingsGroup("Plugins");
    await pluginTab("Repositories").click();
    await $("button=Add repository").click();
    await fill("Repository address", fixtures.thirdPartyUrl ?? "");
    await $("button=Check repository").click();
    const add = $("button.settings-button--danger=Add repository");
    await add.waitForClickable({ timeout: 30_000 });
    await add.click();
    await add.waitForDisplayed({ reverse: true, timeout: 10_000 });

    await pluginTab("Browse").click();
    await card(NAME).$(`button[aria-label='Install ${NAME}']`).waitForClickable({ timeout: 30_000 });
    await openPrompt(NAME);
    // An unofficial plugin may need the "I understand" tick first (FR-9.80); the prompt also
    // lists per-website checkboxes, so only the confirmation box may be ticked here.
    await confirmInstall(NAME);
    await closeSettings();
  });

  after(async () => {
    await pluginenv.stopPluginFixtures();
  });

  it("opens the side panel from a panel click", async () => {
    const icon = $(`//div[contains(@class,'pstrip-icons')]//button[@aria-label='${NAME}']`);
    await icon.waitForClickable({ timeout: 15_000 });
    await icon.click();
    await $(".pstrip-panel button=Open side panel").click();
    await panelLine("open-side: ok").waitForExist({ timeout: 10_000 });
    await sideFrame().waitForExist({ timeout: 15_000 });
    await panelLine("ready: side").waitForExist({ timeout: 15_000 });
  });

  it("refuses the page's request after a script-made click", async () => {
    // Let the panel click's gesture run out, so only the page can supply one.
    await browser.pause(GESTURE_WINDOW_MS + 1_000);
    await inSidePage(async () => {
      await browser.execute(() => {
        window.parent.postMessage({ mumcordWebview: "gesture", key: "guess" }, "*");
        const button = document.getElementById("start-stage");
        button?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
        button?.click();
      });
    });
    await panelLine("side-start-stage: not-permitted").waitForExist({ timeout: 10_000 });
  });

  it("allows it after a real click in the page", async () => {
    await inSidePage(async () => {
      await $("#start-stage").click();
    });
    await panelLine("side-start-stage: ok").waitForExist({ timeout: 10_000 });
  });
});
