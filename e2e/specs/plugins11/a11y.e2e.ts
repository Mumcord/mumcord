// AC-9.39 (spec 09, #395 AC 15, #393 AC 10), the automated part: a keyboard-only pass over every
// new plugin screen of spec 09 in the light and the dark theme. Each screen is reached with Tab
// (or the arrow keys inside a tab list or menu), operated with Enter, Space, Escape, F6 and the
// arrow keys, shows a visible focus ring on what Tab reached, has correct accessible names and has
// no serious or critical axe (WCAG 2.2 AA) violation:
//
//   - Settings → Plugins → Installed: "Plugin safety" (switch) and "Plugin sounds" (volume slider,
//     mute switch);
//   - Browse cards, the details page (score chip, breakdown table, its disclosure, Back);
//   - the install prompt of a High plugin with its score and the one confirmation box;
//   - Updates: Check now and the review of a held update (test-growth 1.1.0, Medium → High) with
//     its tick;
//   - the Apps menu (arrow keys, Enter) starting two stage activities, the stage tabs (arrow keys,
//     Home/End, "<label>, plugin activity, tab n of 2");
//   - the frame chrome: Tab into the stage page and press its tone button, Escape and F6 inside
//     the page return focus to the frame; after a click inside the page (frame sound needs one,
//     FR-9.43) the mute button appears and toggles by keyboard;
//   - the side panel (options menu, resize separator, close, focus return);
//   - the web dialog (focus starts on its content, Shift+Tab stays inside, Escape closes, focus
//     returns to its opener).
//
// The plugins are the fixtures of e2e/fixtures/plugins (third-party fixture repository):
// test-surface, test-stage, test-growth (1.0.0 installed, 1.1.0 published to be held back) and
// test-score-high (only its install prompt is opened).
//
// Manual (not automatable on the Linux runner):
//   - the screen-reader pass on one platform: the chip read as "High risk, score … out of 100,
//     higher means more risk", the breakdown read as a table, a stage tab read as "…, plugin
//     activity, tab 2 of 2" (the spec checks the accessible names and the table markup only);
//   - the high-contrast theme (the runner's GTK has no high-contrast theme the app follows);
//   - the frame chrome on Windows and macOS (native child web views there).
import { copyFileSync, mkdirSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import type { Result } from "axe-core";
import { app, axe, pluginenv, testenv } from "../../helpers/index.ts";
import { addFirstServer, finishWizard, reconnectSaved } from "../../helpers/chat-ui.ts";
import { card, prompt, promptInstall, waitForCard } from "../../helpers/score-ui.ts";
import { openSettingsGroup, settingsCard } from "../../helpers/settings-ui.ts";
import { focus, openSettings, pressUntil, tabTo } from "../app/flows.ts";

type Theme = "light" | "dark";
type El = ReturnType<typeof $>;

const SURFACE = "Test Surface";
const STAGE = "Test Stage";
const GROWTH = "Fixture growth";
const HIGH = "Fixture score-high";
const PANEL_ITEM = "Open Test Surface panel";
/** Held back until the clients run, so the update waits in Updates (Medium → High). */
const HELD_BACK = ["test-growth-1.1.0.mcplugin"];

const cls = (name: string) => `contains(concat(' ',normalize-space(@class),' '),' ${name} ')`;
const updateRow = (name: string) =>
  $(`//li[${cls("plugin-update")}][.//span[contains(@class,'plugin-update__name')][normalize-space(.)='${name}']]`);
const reviewDialog = () => $(`//*[${cls("plugin-review")}]`);
const appsButton = () => $("button.chat-apps__button");
const appsMenu = () => $("div.chat-apps__menu");
const stageTab = (label: string) =>
  $(`//button[@role='tab'][${cls("media-stage__tab")}][.//*[contains(@class,'media-stage__label')][normalize-space()='${label}']]`);
const stageFrame = (plugin: string) => $(`.plugin-stage[aria-label='Media area: ${plugin}']`);
const sidePanel = () => $(`.plugin-sidepanel[aria-label='${SURFACE} side panel']`);
const webDialog = () => $("div.plugin-web-dialog[role='dialog']");
const stripIcon = () => $(`button.pstrip-icon[aria-label='${SURFACE}']`);

/**
 * The text of a `.visually-hidden` span, which only screen readers get. WebDriver's element text
 * is the rendered text and is empty for such a clipped 1 px span, so read its text content.
 */
async function screenReaderText(el: El): Promise<string> {
  await el.waitForExist({ timeout: 10_000 });
  return String(await el.getProperty("textContent"));
}

/** Serious and critical violations and missing focus rings, collected so one run reports all. */
const findings: string[] = [];

function expectNoFindings(): void {
  const all = findings.splice(0);
  expect(all.join("\n")).toBe("");
}

/** Axe on `include` (plugin pages inside frames are the plugins' own and are left out). */
async function check(label: string, include: string[]): Promise<void> {
  // Let transitions settle so colour contrast sees the final colours.
  await browser.pause(400);
  const violations: Result[] = await axe.scan(
    label,
    { iframes: false },
    { include: include.map((s) => [s]) },
  );
  for (const v of violations) {
    if (v.impact !== "serious" && v.impact !== "critical") continue;
    const targets = v.nodes.map((n) => n.target.join(" ")).slice(0, 3).join(" | ");
    findings.push(`${label}: ${v.impact} ${v.id} (${v.help}) → ${targets}`);
  }
}

/**
 * Records a finding when the focused element shows no focus indicator: an outline or shadow on
 * itself, a drawn ::before/::after (the resize handle), or an outline on the label or track next
 * to a visually hidden input (switches).
 */
async function expectFocusRing(label: string): Promise<void> {
  const problem = await browser.execute(() => {
    const el = document.activeElement;
    if (!(el instanceof HTMLElement) || el === document.body) return "nothing has focus";
    const outlined = (e: Element | null) => {
      if (e === null) return false;
      const s = getComputedStyle(e);
      return (s.outlineStyle !== "none" && parseFloat(s.outlineWidth) > 0) || s.boxShadow !== "none";
    };
    const drawn = (pseudo: string) => {
      const s = getComputedStyle(el, pseudo);
      if (s.content === "none" || s.content === "normal") return false;
      return s.backgroundColor !== "rgba(0, 0, 0, 0)" || s.boxShadow !== "none" || s.outlineStyle !== "none";
    };
    const ok =
      outlined(el) ||
      drawn("::before") ||
      drawn("::after") ||
      outlined(el.nextElementSibling) ||
      outlined(el.parentElement);
    const name = el.getAttribute("aria-label") ?? el.innerText.trim().slice(0, 40);
    return ok ? "" : `<${el.tagName.toLowerCase()} class="${el.className}"> "${name}" has no visible focus`;
  });
  if (problem !== "") findings.push(`${label}: ${problem}`);
}

/** Tabs (Shift+Tab when `back`) to `el`, then checks its focus ring. */
async function tabToEl(el: El, what: string, back = false, max = 250): Promise<void> {
  await el.waitForExist({ timeout: 30_000, timeoutMsg: `${what} never showed` });
  await tabTo(() => el.isFocused(), what, back, max);
  await expectFocusRing(what);
}

/** Whether focus sits inside (or on) the element matching `selector`. */
async function focusInside(selector: string): Promise<boolean> {
  return browser.execute((s: string) => document.activeElement?.closest(s) != null, selector);
}

async function expectFocusSomewhere(what: string): Promise<void> {
  const lost = await browser.execute(
    () => document.activeElement === null || document.activeElement === document.body,
  );
  if (lost) findings.push(`${what}: focus was dropped to the page body`);
}

/**
 * The first text line of the focused menu item's label (without its plugin line). Read from the
 * label span, because a plugin without an icon shows a letter tile (aria-hidden) before it.
 */
async function focusedFirstLine(): Promise<string> {
  const text = await browser.execute(() => {
    const e = document.activeElement as HTMLElement | null;
    const label = e?.querySelector<HTMLElement>(".appmenu__text, .chat-apps__text") ?? e;
    return label?.innerText ?? "";
  });
  return (text.split("\n")[0] ?? "").trim();
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

/** Installs a store plugin as a confirmed prompt would and waits until it runs (setup only). */
async function install(id: string): Promise<void> {
  await invokeOk("plugins_index", { language: "en" });
  await invokeOk("plugin_install_prepare", { id });
  await invokeOk("plugin_install", { id, version: null, optional: [], visibility: "nobody", confirmed: true });
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

/** A copy of the built third-party packages without `HELD_BACK`. */
function packagesWithoutHeldBack(): string {
  const root = process.env.MUMCORD_E2E_PLUGIN_PACKAGES;
  if (root === undefined || root === "") throw new Error("MUMCORD_E2E_PLUGIN_PACKAGES is not set");
  const source = join(root, "packages", "repo");
  const target = mkdtempSync(join(tmpdir(), "mumcord-a11y-repo-"));
  mkdirSync(target, { recursive: true });
  for (const file of readdirSync(source)) {
    if (file.endsWith(".mcplugin") && !HELD_BACK.includes(file)) {
      copyFileSync(join(source, file), join(target, file));
    }
  }
  return target;
}

const themeEnv = (theme: Theme) =>
  pluginenv.appEnv({ GTK_THEME: theme === "dark" ? "Adwaita:dark" : "Adwaita" });

async function waitTheme(theme: Theme): Promise<void> {
  await browser.waitUntil(
    async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme,
    { timeout: 15_000, timeoutMsg: `the app did not show the ${theme} theme` },
  );
}

// ---------------------------------------------------------------- keyboard drivers

/** Opens Settings → Plugins with the keyboard (the sidebar selects on focus). */
async function openPluginSettings(): Promise<void> {
  await openSettings();
  await pressUntil(
    "ArrowDown",
    // By id, not text: the tab also shows the pending-updates count ("Plugins 1").
    (f) => f.role === "tab" && f.id.endsWith("-tab-plugins"),
    "the Plugins settings group",
  );
  await $("//h2[contains(@class,'settings-page__title')][normalize-space(.)='Plugins']").waitForDisplayed({
    timeout: 10_000,
  });
}

/** Moves to a Settings → Plugins tab with the arrow keys and checks it got selected. */
async function pluginsTab(name: string): Promise<void> {
  await tabTo((f) => f.role === "tab" && f.cls.includes("plugins-tab"), "the plugin tabs");
  await pressUntil("ArrowRight", (f) => f.role === "tab" && f.text.startsWith(name), `the ${name} tab`, 8);
  await expectFocusRing(`${name} tab`);
  const tab = $(`//button[@role='tab'][contains(@class,'plugins-tab')][starts-with(normalize-space(.),'${name}')]`);
  await expect(tab).toHaveAttribute("aria-selected", "true", { wait: 5_000 });
}

async function closeSettingsByKey(): Promise<void> {
  await browser.keys("Escape");
  await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
}

/** Toggles a switch with Space and back, checking it follows. */
async function toggleBySpace(toggle: El, what: string): Promise<void> {
  await tabToEl(toggle, what);
  const before = await toggle.isSelected();
  await browser.keys([" "]);
  await browser.waitUntil(async () => (await toggle.isSelected()) !== before, {
    timeout: 5_000,
    timeoutMsg: `Space did not switch ${what}`,
  });
  await browser.keys([" "]);
  await browser.waitUntil(async () => (await toggle.isSelected()) === before, {
    timeout: 5_000,
    timeoutMsg: `Space did not switch ${what} back`,
  });
}

/** Opens the Apps menu with Enter and chooses `label` with the arrow keys and Enter. */
async function chooseAppByKey(label: string): Promise<void> {
  await tabToEl(appsButton(), "the Apps button");
  await browser.keys("Enter");
  await appsMenu().waitForDisplayed({ timeout: 5_000 });
  await browser.waitUntil(() => focusInside("div.chat-apps__menu"), {
    timeout: 5_000,
    timeoutMsg: "the Apps menu did not take focus",
  });
  const seen: string[] = [];
  for (let i = 0; i < 30; i++) {
    const f = await focus();
    const line = await focusedFirstLine();
    if (f.role === "menuitem" && line === label) {
      await expectFocusRing(`Apps menu item ${label}`);
      await browser.keys("Enter");
      await appsMenu().waitForExist({ reverse: true, timeout: 5_000 });
      await expectFocusSomewhere(`after choosing ${label}`);
      return;
    }
    seen.push(line);
    await browser.keys("ArrowDown");
  }
  throw new Error(`the arrow keys never reached "${label}" in the Apps menu; saw: ${seen.join(" | ")}`);
}

// ---------------------------------------------------------------- the screens

async function settingsScreens(theme: Theme): Promise<void> {
  await openPluginSettings();

  // Installed: Plugin safety and Plugin sounds.
  await pluginsTab("Installed");
  await check(`${theme}-installed`, [".overlay__dialog"]);
  await toggleBySpace(settingsCard("Plugin safety").$("input[role='switch']"), "Don't allow high-risk plugins");
  const volume = settingsCard("Plugin sounds").$("input[type='range']");
  await tabToEl(volume, "Plugin sound volume");
  const level = Number(await volume.getValue());
  await browser.keys(level >= 100 ? "ArrowLeft" : "ArrowRight");
  await browser.waitUntil(async () => Number(await volume.getValue()) !== level, {
    timeout: 5_000,
    timeoutMsg: "the arrow keys did not change the plugin sound volume",
  });
  await browser.keys(level >= 100 ? "ArrowRight" : "ArrowLeft");
  await browser.waitUntil(async () => Number(await volume.getValue()) === level, {
    timeout: 5_000,
    timeoutMsg: "the plugin sound volume did not go back",
  });
  await toggleBySpace(settingsCard("Plugin sounds").$("input[role='switch']"), "Mute plugin sounds");

  // Browse: the card, then its details page.
  await pluginsTab("Browse");
  await waitForCard(HIGH);
  await check(`${theme}-browse`, [".overlay__dialog"]);
  expect(await screenReaderText(card(HIGH).$(".risk-chip .visually-hidden"))).toMatch(
    /^Security score \d+ out of 100\. High risk\. Higher means more risk\.$/,
  );
  await tabToEl(card(HIGH).$(`button[aria-label='Details of ${HIGH}']`), `Details of ${HIGH}`);
  await browser.keys("Enter");
  const score = $("section.pd-score");
  await score.waitForDisplayed({ timeout: 30_000, timeoutMsg: "the details page did not open" });
  // Each group of the breakdown is a real table with header cells, which a screen reader reads as one.
  const tables = await score.$$("table.risk-table");
  expect(tables.length).toBeGreaterThan(0);
  for (const table of tables) expect((await table.$$("th")).length).toBeGreaterThan(0);
  await check(`${theme}-details`, [".overlay__dialog"]);
  const summary = score.$("details.risk-details > summary");
  await tabToEl(summary, "the score breakdown disclosure");
  const disclosure = score.$("details.risk-details");
  const open = (await disclosure.getAttribute("open")) !== null;
  await browser.keys("Enter");
  await browser.waitUntil(async () => ((await disclosure.getAttribute("open")) !== null) !== open, {
    timeout: 5_000,
    timeoutMsg: "Enter did not toggle the score breakdown",
  });
  await browser.keys("Enter");
  await tabToEl($("button.pd-back"), "Back to Browse");
  await browser.keys("Enter");
  await waitForCard(HIGH);
  await expectFocusSomewhere("back from the details page");

  // The install prompt with its score and the one confirmation box.
  await tabToEl(card(HIGH).$(`button[aria-label='Install ${HIGH}']`), `Install ${HIGH}`);
  await browser.keys("Enter");
  await prompt().waitForDisplayed({ timeout: 30_000, timeoutMsg: "the install prompt did not open" });
  await prompt().$(".plugin-prompt-score").waitForExist({ timeout: 60_000, timeoutMsg: "the prompt never showed its score" });
  await browser.waitUntil(() => focusInside(".plugin-prompt"), {
    timeout: 5_000,
    timeoutMsg: "the install prompt did not take focus",
  });
  expect(await screenReaderText(prompt().$(".risk-chip .visually-hidden"))).toMatch(
    /out of 100\. High risk\. Higher means more risk\.$/,
  );
  await check(`${theme}-prompt`, [".plugin-prompt"]);
  const tick = prompt().$(".plugin-prompt-confirm input[type='checkbox']");
  await tabToEl(tick, "the confirmation box", false, 40);
  await expect(promptInstall()).toBeDisabled();
  await browser.keys([" "]);
  await expect(promptInstall()).toBeEnabled({ wait: 5_000 });
  await tabToEl(promptInstall(), "Install anyway", false, 20);
  await tabToEl(tick, "the confirmation box again", true, 20);
  await browser.keys([" "]);
  await expect(promptInstall()).toBeDisabled({ wait: 5_000 });
  await browser.keys("Escape");
  await prompt().waitForExist({ reverse: true, timeout: 10_000 });
  expect(await $(".settings__nav").isExisting()).toBe(true);
  await expectFocusSomewhere("after closing the install prompt");

  // Updates: Check now, then the held update's review with its tick.
  await pluginsTab("Updates");
  await tabToEl($("//button[normalize-space()='Check now']"), "Check now");
  await browser.keys("Enter");
  await expect($("p.plugin-updates__checked")).toHaveText(expect.stringContaining("Last checked"), {
    wait: 60_000,
  });
  await updateRow(GROWTH).waitForDisplayed({ timeout: 30_000, timeoutMsg: "test-growth 1.1.0 is not held in Updates" });
  await check(`${theme}-updates`, [".overlay__dialog"]);
  await tabToEl(updateRow(GROWTH).$("button*=Review and update"), "Review and update");
  await browser.keys("Enter");
  const review = reviewDialog();
  await review.waitForDisplayed({ timeout: 10_000 });
  await check(`${theme}-review`, [".plugin-review"]);
  const update = review.$("button=Update anyway");
  await tabToEl(review.$(".plugin-review__tick input[type='checkbox']"), "the update's confirmation box", false, 40);
  await expect(update).toBeDisabled();
  await browser.keys([" "]);
  await expect(update).toBeEnabled({ wait: 5_000 });
  await browser.keys([" "]);
  await expect(update).toBeDisabled({ wait: 5_000 });
  await browser.keys("Escape");
  await review.waitForDisplayed({ reverse: true, timeout: 10_000 });
  await expectFocusSomewhere("after closing the update review");

  await closeSettingsByKey();
}

async function appsMenuAndStageTabs(theme: Theme): Promise<void> {
  // The menu itself, scanned while open.
  await tabToEl(appsButton(), "the Apps button");
  await browser.keys("Enter");
  await appsMenu().waitForDisplayed({ timeout: 5_000 });
  await check(`${theme}-apps-menu`, ["div.chat-apps__menu"]);
  await browser.keys("Escape");
  await appsMenu().waitForExist({ reverse: true, timeout: 5_000 });
  await expect(appsButton()).toBeFocused();

  await chooseAppByKey(`Start ${STAGE}`);
  // A single item has no tab row (the tabs appear from two items on), only its stage.
  await stageFrame(STAGE).waitForExist({ timeout: 20_000, timeoutMsg: "Test Stage did not start" });
  await chooseAppByKey(`Start ${SURFACE}`);
  await stageTab(SURFACE).waitForExist({ timeout: 20_000, timeoutMsg: "Test Surface did not start" });

  // Two plugin activities: two tabs, named for a screen reader.
  for (const label of [STAGE, SURFACE]) {
    await expect(stageTab(label)).toHaveAttribute(
      "aria-label",
      expect.stringMatching(new RegExp(`^${label}, plugin activity, (new, )?tab [12] of 2$`)),
    );
  }
  await expect($("div.media-stage__tabs[role='tablist']")).toHaveAttribute("aria-label", "What the media area shows");

  // Looked up again on every use: once found, an element stays that button, but the selection moves.
  const selected = () => $("//button[@role='tab'][contains(@class,'media-stage__tab')][@aria-selected='true']");
  await tabToEl(selected(), "the selected stage tab");
  const first = (await selected().getAttribute("aria-label")) ?? "";
  await browser.keys("ArrowRight");
  await browser.waitUntil(async () => ((await selected().getAttribute("aria-label")) ?? "") !== first, {
    timeout: 5_000,
    timeoutMsg: "ArrowRight did not select the other stage tab",
  });
  expect(await selected().isFocused()).toBe(true);
  await browser.keys("Home");
  await expect(selected()).toHaveAttribute("aria-label", expect.stringMatching(/tab 1 of 2$/), { wait: 5_000 });
  await browser.keys("End");
  await expect(selected()).toHaveAttribute("aria-label", expect.stringMatching(/tab 2 of 2$/), { wait: 5_000 });
  await check(`${theme}-stage-tabs`, [".media-stage__row"]);

  // Leave Test Surface's page on the stage for the frame chrome.
  await pressUntil(
    "ArrowLeft",
    (f) => f.role === "tab" && f.label.startsWith(`${SURFACE},`),
    "the Test Surface tab",
    4,
  );
  await stageFrame(SURFACE).waitForDisplayed({ timeout: 10_000 });
}

async function frameChrome(theme: Theme): Promise<void> {
  const frame = stageFrame(SURFACE);
  const page = frame.$("iframe.plugin-webview__frame");
  const wrapper = frame.$(".plugin-webview");
  await page.waitForExist({ timeout: 15_000 });
  await expect(page).toHaveAttribute("title", `${SURFACE}, plugin content`);
  await expect(frame.$(".plugin-frame__tag")).toHaveText(`Plugin content · ${SURFACE} · Unofficial`);

  // Tab into the page and press Enter on its tone button: a key press inside is a gesture
  // (FR-9.45), though only a click unlocks frame sound (FR-9.43), see the mute button below.
  await tabTo(() => page.isFocused(), "the stage page", false, 250);
  await browser.switchFrame(page);
  try {
    await tabTo((f) => f.id === "tone", "the page's tone button", false, 10);
    await browser.keys("Enter");
    await browser.keys("Escape");
  } finally {
    await browser.switchFrame(null);
  }
  await browser.waitUntil(() => wrapper.isFocused(), {
    timeout: 5_000,
    timeoutMsg: "Escape inside the page did not return focus to the frame",
  });

  // F6 leaves the page as well.
  await browser.keys("Tab");
  await browser.switchFrame(page);
  try {
    await tabTo((f) => f.tag === "button", "a button of the page", false, 10);
    await browser.keys("F6");
  } finally {
    await browser.switchFrame(null);
  }
  await browser.waitUntil(() => wrapper.isFocused(), {
    timeout: 5_000,
    timeoutMsg: "F6 inside the page did not return focus to the frame",
  });

  // Frame sound starts only after a click inside the page (FR-9.43, AC-9.24), so a mouse click on
  // the tone button makes the chrome show the mute button; Escape brings focus back to the frame
  // and the button then toggles by keyboard.
  await browser.switchFrame(page);
  try {
    const tone = $("#tone");
    await tone.waitForClickable({ timeout: 10_000 });
    await tone.click();
    await browser.keys("Escape");
  } finally {
    await browser.switchFrame(null);
  }
  await browser.waitUntil(() => wrapper.isFocused(), {
    timeout: 5_000,
    timeoutMsg: "Escape inside the page did not return focus to the frame",
  });
  const mute = frame.$(`button[aria-label='Mute sounds from ${SURFACE}']`);
  await mute.waitForExist({ timeout: 15_000, timeoutMsg: "the frame chrome shows no mute button" });
  await tabToEl(mute, "the frame's mute button", true);
  await browser.keys("Enter");
  const unmute = frame.$(`button[aria-label='Unmute sounds from ${SURFACE}']`);
  await unmute.waitForExist({ timeout: 5_000, timeoutMsg: "Enter did not mute the frame" });
  expect(await unmute.isFocused()).toBe(true);
  await check(`${theme}-frame-chrome`, [`.plugin-stage[aria-label='Media area: ${SURFACE}']`]);
  await browser.keys("Enter");
  await mute.waitForExist({ timeout: 5_000, timeoutMsg: "Enter did not unmute the frame" });
}

async function sidePanelScreen(theme: Theme): Promise<void> {
  await chooseAppByKey(PANEL_ITEM);
  const panel = sidePanel();
  await panel.waitForDisplayed({ timeout: 15_000, timeoutMsg: "the side panel did not open" });
  await check(`${theme}-side-panel`, [".plugin-sidepanel"]);

  const options = panel.$(`button[aria-label='${SURFACE} options']`);
  await tabToEl(options, "the side panel's options button");
  await browser.keys("Enter");
  await expect(options).toHaveAttribute("aria-expanded", "true", { wait: 5_000 });
  await browser.keys("Escape");
  await expect(options).toHaveAttribute("aria-expanded", "false", { wait: 5_000 });
  await expectFocusSomewhere("after closing the side panel's options");

  const handle = $(`.resize-handle[role='separator'][aria-label='Resize ${SURFACE} side panel']`);
  await tabToEl(handle, "the side panel's resize handle");
  const width = Number(await handle.getAttribute("aria-valuenow"));
  await browser.keys("ArrowLeft");
  await browser.waitUntil(async () => Number(await handle.getAttribute("aria-valuenow")) !== width, {
    timeout: 5_000,
    timeoutMsg: "ArrowLeft did not resize the side panel",
  });
  await browser.keys("ArrowRight");
  await browser.waitUntil(async () => Number(await handle.getAttribute("aria-valuenow")) === width, {
    timeout: 5_000,
    timeoutMsg: "ArrowRight did not resize the side panel back",
  });

  await tabToEl(panel.$(`button[aria-label='Close ${SURFACE} side panel']`), "the side panel's close button");
  await browser.keys("Enter");
  await panel.waitForExist({ reverse: true, timeout: 10_000 });
  await expectFocusSomewhere("after closing the side panel");
}

async function dialogScreen(theme: Theme): Promise<void> {
  // The strip is a toolbar: Tab reaches one icon (roving tabindex), the arrow keys the others.
  await tabTo((f) => f.cls.includes("pstrip-icon"), "the plugin strip");
  await pressUntil("ArrowDown", (f) => f.cls.includes("pstrip-icon") && f.label === SURFACE, `the ${SURFACE} strip icon`, 8);
  if ((await stripIcon().getAttribute("aria-pressed")) !== "true") await browser.keys("Enter");
  await $(".pstrip-body").waitForExist({ timeout: 10_000, timeoutMsg: "the strip panel did not open" });
  await check(`${theme}-strip`, [".pstrip"]);
  const opener = $("//*[contains(@class,'pstrip-body')]//button[normalize-space()='Open dialog']");
  await tabToEl(opener, "Open dialog");
  await browser.keys("Enter");

  const dialog = webDialog();
  await dialog.waitForDisplayed({ timeout: 15_000, timeoutMsg: "the web dialog did not open" });
  await browser.waitUntil(() => focusInside(".plugin-web-dialog"), {
    timeout: 5_000,
    timeoutMsg: "the web dialog did not take focus",
  });
  const titleId = (await dialog.getAttribute("aria-labelledby")) ?? "";
  // The title may be for screen readers only, so read its text content, not the rendered text.
  expect((await screenReaderText($(`[id='${titleId}']`))).trim()).not.toBe("");
  await check(`${theme}-dialog`, [".plugin-web-dialog"]);

  // Shift+Tab stays inside the dialog (its sentinels wrap around).
  for (let i = 0; i < 4; i++) {
    await browser.keys(["Shift", "Tab"]);
    if (!(await focusInside(".plugin-web-dialog"))) {
      throw new Error(`Shift+Tab left the web dialog: ${JSON.stringify(await focus())}`);
    }
  }
  await tabToEl(dialog.$(`button[aria-label='Close ${SURFACE}']`), "the dialog's close button", true, 20);
  await browser.keys("Escape");
  await dialog.waitForExist({ reverse: true, timeout: 10_000 });
  await browser.waitUntil(() => opener.isFocused(), {
    timeout: 5_000,
    timeoutMsg: "closing the web dialog did not return focus to Open dialog",
  });
}

// ---------------------------------------------------------------- the spec

describe("plugins 1.1: keyboard and themes over the new plugin UI (AC-9.39)", function () {
  this.timeout(30 * 60_000);
  let repo = "";

  before(async function () {
    this.timeout(20 * 60_000);
    repo = packagesWithoutHeldBack();
    const fixtures = await pluginenv.startPluginFixtures({ thirdParty: { packages: repo }, https: true });
    await testenv.up("v15");
    await app.restart({ env: themeEnv("light") });
    await finishWizard("Alice");
    await addFirstServer(testenv.ports.v15);
    // A fresh connect is read-only (AC-1.35); join so the Apps menu shows.
    const join = $(".userpanel-readonly__join");
    await join.waitForClickable({ timeout: 15_000 });
    await join.click();
    await $(".userpanel-readonly").waitForExist({ reverse: true, timeout: 15_000 });
    // The fixture repository is plain http on localhost, which only Developer mode accepts.
    await openSettingsGroup("Advanced & Developer");
    const developer = $("//input[@role='switch'][@id=//label[.='Developer mode']/@for]");
    await developer.waitForExist({ timeout: 10_000 });
    await developer.click();
    await expect(developer).toBeSelected();
    await closeSettingsByKey();
    await browser.waitUntil(
      async () => (await call("plugin_repo_add", { url: fixtures.thirdPartyUrl, expectedKey: null })).error === undefined,
      { timeout: 10_000, timeoutMsg: "the fixture repository could not be added" },
    );
    for (const id of ["test-surface", "test-stage", "test-growth"]) await install(id);
    await pluginenv.publish(pluginenv.stagedPackage("test-growth-1.1.0.mcplugin"), "third-party");
    await waitTheme("light");
  });

  after(async () => {
    await pluginenv.stopPluginFixtures();
    if (repo !== "") rmSync(repo, { recursive: true, force: true });
  });

  for (const theme of ["light", "dark"] as const) {
    it(`${theme}: Plugin safety, Plugin sounds, Browse, details, install prompt and Updates by keyboard`, async () => {
      if (theme === "dark") {
        await app.restart({ env: themeEnv("dark") });
        await reconnectSaved();
        await waitTheme("dark");
      }
      await settingsScreens(theme);
      expectNoFindings();
    });

    it(`${theme}: the Apps menu and the stage tabs by keyboard`, async () => {
      await appsMenuAndStageTabs(theme);
      expectNoFindings();
    });

    it(`${theme}: the frame chrome (Escape and F6 out of the page, the mute button)`, async () => {
      await frameChrome(theme);
      expectNoFindings();
    });

    it(`${theme}: the side panel by keyboard`, async () => {
      await sidePanelScreen(theme);
      expectNoFindings();
    });

    it(`${theme}: the web dialog keeps and returns focus`, async () => {
      await dialogScreen(theme);
      expectNoFindings();
    });
  }
});
