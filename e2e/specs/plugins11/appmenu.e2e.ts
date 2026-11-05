// Spec 09 (plugin platform 1.1), the Apps menu, the command palette and the web frames of
// `test-surface` (third-party fixture repository, scripts/e2e-plugin-registry.sh):
//
//   AC-9.16  the side panel and the dialog open after a gesture with the frame chrome and the
//            "Plugin content" tag; a fourth frame is refused (quota-exceeded, the V1a deviation
//            from the spec's `limit`); the side panel resizes by keyboard and keeps its width
//            after a restart.
//   AC-9.17  plugin items under "Plugins" with icon and name, in the command palette as
//            "<plugin>: <label>"; choosing one delivers app-menu-activated; set-state disables an
//            item and shows the reason; test-badlabel (an Apps-menu label equal to a built-in one)
//            does not load and is listed as refused (Diagnostics).
//
// Manual: the frame chrome on Windows and macOS (native child webviews there).
import { $, $$, browser, expect } from "@wdio/globals";
import { app, pluginenv, testenv } from "../../helpers/index.ts";
import { addFirstServer, finishWizard, reconnectSaved } from "../../helpers/chat-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";

const SURFACE = "Test Surface";
const PANEL_ITEM = "Open Test Surface panel";
const TAG = `Plugin content · ${SURFACE} · Unofficial`;

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

async function pluginStatus(id: string): Promise<string | undefined> {
  const list = (await invokeOk("plugins_list")) as {
    plugins: { id: string; status: { kind: string } }[];
  };
  return list.plugins.find((p) => p.id === id)?.status.kind;
}

/** Installs a store plugin as the prompt would (prepare, then install). */
async function install(id: string): Promise<{ error?: string }> {
  await invokeOk("plugins_index", { language: "en" });
  const prepared = await call("plugin_install_prepare", { id });
  if (prepared.error !== undefined) return prepared;
  return call("plugin_install", { id, version: null, optional: [], visibility: "nobody", confirmed: true });
}

function appsMenu() {
  return $("div.chat-apps__menu");
}

async function openAppsMenu(): Promise<void> {
  const button = $("button.chat-apps__button");
  await button.waitForClickable({ timeout: 20_000 });
  await button.click();
  await appsMenu().waitForDisplayed({ timeout: 5_000 });
}

async function closeAppsMenu(): Promise<void> {
  await browser.keys("Escape");
  await appsMenu().waitForExist({ reverse: true, timeout: 5_000 });
}

/** A plugin's item in the open Apps menu, by its exact label. */
function appItem(label: string) {
  return $(
    `//div[contains(@class,'chat-apps__menu')]//button[contains(@class,'appmenu__item')][.//span[contains(@class,'appmenu__text')]/text()[normalize-space()='${label}']]`,
  );
}

async function chooseApp(label: string): Promise<void> {
  await openAppsMenu();
  const item = appItem(label);
  await item.waitForClickable({ timeout: 20_000, timeoutMsg: `the Apps menu has no "${label}"` });
  await item.click();
  await appsMenu().waitForExist({ reverse: true, timeout: 5_000 });
}

async function openPanel(): Promise<void> {
  const icon = $(`button.pstrip-icon[aria-label='${SURFACE}']`);
  await icon.waitForExist({ timeout: 20_000, timeoutMsg: `${SURFACE} has no strip icon` });
  // An open panel is left alone: reading its log must work while a plugin dialog covers it.
  if ((await icon.getAttribute("aria-pressed")) !== "true") {
    await icon.waitForClickable({ timeout: 10_000 });
    await icon.click();
  }
}

/** Clicks a button of the plugin's panel (a user gesture for the plugin). */
async function panelButton(label: string): Promise<void> {
  await openPanel();
  const button = $(`//*[contains(@class,'pstrip-body')]//button[normalize-space()='${label}']`);
  await button.waitForClickable({ timeout: 10_000, timeoutMsg: `the panel has no "${label}"` });
  await button.click();
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

/** Waits until the plugin's panel shows a line matching `pattern`; returns that line. */
async function waitLog(pattern: RegExp, timeout = 15_000): Promise<string> {
  await openPanel();
  let seen = "";
  let line = "";
  // timeoutMsg is fixed when the wait starts, so the panel text seen last is reported by hand.
  await browser
    .waitUntil(
      async () => {
        seen = await panelLog();
        line = seen.split("\n").find((l) => pattern.test(l)) ?? "";
        return line !== "";
      },
      { timeout, interval: 250 },
    )
    .catch(() => {
      throw new Error(`the panel never showed ${String(pattern)}; it shows:\n${seen}`);
    });
  return line;
}

function sidePanel() {
  return $(`.plugin-sidepanel[aria-label='${SURFACE} side panel']`);
}

function separator() {
  return $(`.resize-handle[role='separator'][aria-label='Resize ${SURFACE} side panel']`);
}

async function openPalette(): Promise<void> {
  await browser.keys(["Control", "Shift", "p"]);
  await $("input[role='combobox'][aria-label='Command palette']").waitForDisplayed({ timeout: 5_000 });
}

async function paletteOptions(): Promise<string[]> {
  const texts: string[] = [];
  for (const option of await $$("div.plugins-palette__list div[role='option']")) {
    texts.push((await option.getText()).trim());
  }
  return texts;
}

describe("plugins 1.1: Apps menu, command palette and web frames (AC-9.16, AC-9.17)", function () {
  this.timeout(20 * 60_000);

  before(async function () {
    this.timeout(15 * 60_000);
    const fixtures = await pluginenv.startPluginFixtures({ thirdParty: {}, https: true });
    await testenv.up("v15");
    await app.restart({ env: pluginenv.appEnv() });
    await finishWizard("Alice");
    await addFirstServer(testenv.ports.v15);
    const join = $(".userpanel-readonly__join");
    await join.waitForClickable({ timeout: 15_000 });
    await join.click();
    // The fixture repository is plain http on localhost, which only Developer mode accepts.
    await openSettingsGroup("Advanced & Developer");
    const developer = $("//input[@role='switch'][@id=//label[.='Developer mode']/@for]");
    await developer.waitForExist({ timeout: 10_000 });
    await developer.click();
    await expect(developer).toBeSelected();
    await browser.keys("Escape");
    await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
    await browser.waitUntil(
      async () =>
        (await call("plugin_repo_add", { url: fixtures.thirdPartyUrl, expectedKey: null })).error ===
        undefined,
      { timeout: 10_000, timeoutMsg: "the fixture repository could not be added" },
    );
    expect((await install("test-surface")).error).toBeUndefined();
    await browser.waitUntil(async () => (await pluginStatus("test-surface")) === "running", {
      timeout: 30_000,
      timeoutMsg: "test-surface never started",
    });
  });

  after(async () => {
    await pluginenv.stopPluginFixtures();
  });

  it("AC-9.17: plugin items sit under Plugins with icon and name", async () => {
    await openAppsMenu();
    try {
      await expect($("//div[contains(@class,'chat-apps__menu')]//p[contains(@class,'appmenu__group')]")).toHaveText(
        "Plugins",
        { ignoreCase: true },
      );
      for (const label of ["Start Test Surface", PANEL_ITEM]) {
        const item = appItem(label);
        await item.waitForDisplayed({ timeout: 10_000, timeoutMsg: `no "${label}" item` });
        await expect(item.$(".appmenu__icon")).toBeDisplayed();
        await expect(item.$(".appmenu__sub")).toHaveText(`${SURFACE} · Unofficial`);
        expect(await item.getAttribute("aria-disabled")).toBeNull();
      }
    } finally {
      await closeAppsMenu();
    }
  });

  it("AC-9.17: the command palette lists the items and choosing one reaches the plugin", async () => {
    await openPalette();
    const option = `${SURFACE}: ${PANEL_ITEM}`;
    await browser.waitUntil(async () => (await paletteOptions()).includes(option), {
      timeout: 5_000,
      timeoutMsg: `the palette has no "${option}"`,
    });
    await $(`//div[@role='option'][normalize-space()='${option}']`).click();
    // app-menu-activated "side" opens the side panel (and is the gesture it needs).
    await waitLog(/^open-side: ok$/);
    await sidePanel().waitForDisplayed({ timeout: 15_000 });
    await $(`button[aria-label='Close ${SURFACE} side panel']`).click();
    await sidePanel().waitForExist({ reverse: true, timeout: 10_000 });
  });

  it("AC-9.17: set-state disables an item with its reason and enables it again", async () => {
    await panelButton("Disable panel item");
    await waitLog(/^item-disable: ok$/);
    await openAppsMenu();
    try {
      const item = appItem(PANEL_ITEM);
      await expect(item).toHaveAttribute("aria-disabled", "true", { wait: 10_000 });
      await expect(item.$(".chat-apps__reason")).toHaveText("Disabled by the test");
      // A disabled item does nothing.
      await item.click();
    } finally {
      if (await appsMenu().isExisting()) await closeAppsMenu();
    }
    expect(await sidePanel().isExisting()).toBe(false);
    await openPalette();
    expect(await paletteOptions()).not.toContain(`${SURFACE}: ${PANEL_ITEM}`);
    await browser.keys("Escape");

    await panelButton("Enable panel item");
    await waitLog(/^item-enable: ok$/);
    await openAppsMenu();
    try {
      await browser.waitUntil(async () => (await appItem(PANEL_ITEM).getAttribute("aria-disabled")) === null, {
        timeout: 10_000,
        timeoutMsg: "the item stayed disabled",
      });
      await expect(appItem(PANEL_ITEM).$(".chat-apps__reason")).not.toBeExisting();
    } finally {
      await closeAppsMenu();
    }
  });

  it("AC-9.16: dialog and side panel show the chrome and the tag; a fourth frame is refused", async () => {
    await chooseApp("Start Test Surface");
    await waitLog(/^start-stage: ok$/);

    await panelButton("Open dialog");
    const dialog = $(`div.plugin-web-dialog[role='dialog']`);
    await dialog.waitForDisplayed({ timeout: 15_000, timeoutMsg: "the web dialog did not open" });
    await expect(dialog.$(".plugin-frame__tag")).toHaveText(TAG);
    await expect(dialog.$(`button[aria-label='Close ${SURFACE}']`)).toBeDisplayed();
    // On Linux the frame is an iframe, named by its title (the native web views elsewhere use aria-label).
    await expect(dialog.$("iframe.plugin-webview__frame")).toHaveAttribute("title", `${SURFACE}, plugin content`);
    await dialog.$(`button[aria-label='Close ${SURFACE}']`).click();
    await dialog.waitForExist({ reverse: true, timeout: 10_000 });

    await panelButton("Open side panel");
    await sidePanel().waitForDisplayed({ timeout: 15_000 });
    await expect(sidePanel().$(".plugin-frame__tag")).toHaveText(TAG);
    await expect(sidePanel().$(`button[aria-label='${SURFACE} options']`)).toBeDisplayed();
    await expect(sidePanel().$(`button[aria-label='Close ${SURFACE} side panel']`)).toBeDisplayed();

    // Stage, side panel and dialog page are three frames; the panel popover, which shares the
    // dialog's slot, would be the fourth. (A popover opened first closes when the dialog opens.)
    await panelButton("Open dialog and popover");
    await waitLog(/^open-dialog: ok$/);
    const refused = await waitLog(/^open-popover: (?!ok$)/);
    expect(refused).toMatch(/quota/i);
    await expect($(`div.plugin-web-dialog[role='dialog']`)).toBeDisplayed();
    // Close the dialog again.
    if (await $("[role='dialog'][aria-modal='true']").isExisting()) await browser.keys("Escape");
  });

  it("AC-9.16: the side panel resizes by keyboard and keeps its width after a restart", async () => {
    const handle = separator();
    await handle.waitForDisplayed({ timeout: 10_000 });
    const before = Number(await handle.getAttribute("aria-valuenow"));
    await handle.click();
    for (let i = 0; i < 4; i++) await browser.keys("ArrowLeft");
    await browser.waitUntil(async () => Number(await handle.getAttribute("aria-valuenow")) > before, {
      timeout: 5_000,
      timeoutMsg: "ArrowLeft did not widen the side panel",
    });
    const wider = Number(await handle.getAttribute("aria-valuenow"));
    await browser.keys("ArrowRight");
    await browser.waitUntil(async () => Number(await handle.getAttribute("aria-valuenow")) < wider, {
      timeout: 5_000,
      timeoutMsg: "ArrowRight did not narrow the side panel",
    });
    const width = Number(await handle.getAttribute("aria-valuenow"));

    await app.restart({ env: pluginenv.appEnv() });
    await reconnectSaved();
    await browser.waitUntil(async () => (await pluginStatus("test-surface")) === "running", {
      timeout: 30_000,
      timeoutMsg: "test-surface did not start after the restart",
    });
    await chooseApp(PANEL_ITEM);
    await sidePanel().waitForDisplayed({ timeout: 15_000 });
    expect(Number(await separator().getAttribute("aria-valuenow"))).toBe(width);
  });

  it("AC-9.17: a plugin with a built-in Apps-menu label does not load", async () => {
    const result = await install("test-badlabel");
    expect(result.error).toBeDefined();
    expect(await pluginStatus("test-badlabel")).not.toBe("running");
    // A plugin refused at its first load has no record, so Diagnostics lists it from memory.
    const overview = (await invokeOk("plugins_list")) as {
      refused: { name: string; issue: { kind: string } }[];
    };
    const entry = overview.refused.find((refused) => refused.name === "test-badlabel");
    expect(entry?.issue.kind).toBe("refused_app_menu_label");
  });
});
