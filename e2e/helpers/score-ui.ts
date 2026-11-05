// UI helpers for the security-score specs (spec 09 §4.11, §4.12): Settings → Plugins tabs, the
// third-party repository dialog, Browse cards, the install prompt and the Installed rows.
import { $, browser, expect } from "@wdio/globals";
import { fill } from "./servers-ui.ts";
import { openSettingsGroup, turnOnDeveloperMode } from "./settings-ui.ts";

export type PluginsTab = "Installed" | "Browse" | "Updates" | "Repositories";

/** XPath test for one class token, so `risk-chip` does not also match `risk-chip-group`. */
const cls = (name: string) => `contains(concat(' ',normalize-space(@class),' '),' ${name} ')`;

export const pluginTab = (name: PluginsTab) =>
  $(
    `//button[@role='tab'][contains(@class,'plugins-tab')][starts-with(normalize-space(.),'${name}')]`,
  );

export async function openPlugins(tab: PluginsTab): Promise<void> {
  await openSettingsGroup("Plugins");
  await pluginTab(tab).click();
}

export async function closeSettings(): Promise<void> {
  await browser.keys(["Escape"]);
  await $(".overlay__dialog").waitForDisplayed({ reverse: true, timeout: 10_000 });
}

/**
 * Turns on Developer mode (the fixture repositories serve plain http on 127.0.0.1) and adds the
 * third-party fixture repository, optionally with its "Expected key".
 */
export async function addRepository(url: string, expectedKey?: string): Promise<void> {
  await turnOnDeveloperMode();
  await openPlugins("Repositories");
  await $("button=Add repository").click();
  await fill("Repository address", url);
  if (expectedKey !== undefined) await fill("Expected key (optional)", expectedKey);
  await $("button=Check repository").click();
  const confirm = $("button.settings-button--danger=Add repository");
  await confirm.waitForClickable({ timeout: 30_000 });
  await confirm.click();
  await confirm.waitForDisplayed({ reverse: true, timeout: 10_000 });
}

/** A Browse card by the plugin's display name. */
export const card = (name: string) =>
  $(`//li[${cls("plugin-card")}][.//h3[${cls("plugin-card-name")}][contains(.,'${name}')]]`);

/** Waits until Browse shows the card of `name`. */
export async function waitForCard(name: string): Promise<void> {
  await card(name).waitForDisplayed({
    timeout: 30_000,
    timeoutMsg: `${name} never appeared in Browse`,
  });
}

type Scope = ReturnType<typeof $>;

/** The W3C web element reference that `browser.execute` turns into the DOM element. */
export type ElementRef = { "element-6066-11e4-a52e-4f735466cecf": string };

/**
 * The reference of `scope` for `browser.execute`. Built by hand: webdriverio passes a found
 * element it does not recognise as loaded as a plain object, and the page then gets no element.
 */
export async function elementRef(scope: Scope): Promise<ElementRef> {
  const element = await scope.getElement();
  if (!element.elementId) throw new Error("the element for browser.execute was not found");
  return { "element-6066-11e4-a52e-4f735466cecf": element.elementId };
}

/**
 * The visible chip text ("Medium risk · 45", "Official · reviewed", …), without the words only a
 * screen reader gets.
 */
export async function chipText(scope: Scope): Promise<string> {
  const chip = scope.$(`.//span[${cls("risk-chip")}]`);
  await chip.waitForDisplayed({ timeout: 30_000 });
  const text = await browser.execute(
    (el) => {
      const clone = (el as unknown as Element).cloneNode(true) as Element;
      for (const hidden of clone.querySelectorAll(".visually-hidden")) hidden.remove();
      return clone.textContent;
    },
    (await elementRef(chip)) as unknown as HTMLElement,
  );
  return text.replace(/\s+/g, " ").trim();
}

/** The "✓ Official · reviewed" label official Browse cards keep in their title row (FR-9.77). */
export const officialLabel = (scope: Scope | WebdriverIO.Element) =>
  scope.$(`.//span[@class='plugin-label']`);

/** The chip's band (`data-level`: low, medium, high, official, blocked). */
export async function chipLevel(scope: Scope): Promise<string | null> {
  return scope.$(`.//span[${cls("risk-chip")}]`).getAttribute("data-level");
}

/** The number on the chip, or `null` when it shows none. */
export async function chipScore(scope: Scope): Promise<number | null> {
  const score = scope.$(`.//span[${cls("risk-chip__score")}]`);
  if (!(await score.isExisting())) return null;
  const digits = /\d+/.exec(await score.getText())?.[0];
  return digits === undefined ? null : Number(digits);
}

export const prompt = () => $(`//div[${cls("plugin-prompt")}]`);

/** The prompt's primary button ("Install" or "Install anyway"). */
export const promptInstall = () =>
  prompt().$(`.//div[${cls("plugin-prompt-actions")}]//button[${cls("settings-button--primary")}]`);

/**
 * Opens the install prompt from a Browse card and waits until the package was checked: a score
 * (unofficial), the permission intro (official plugins have no score), a refusal or an error.
 */
export async function openPrompt(name: string): Promise<void> {
  await card(name).$(`button[aria-label='Install ${name}']`).click();
  await prompt().waitForDisplayed({ timeout: 30_000 });
  await browser.waitUntil(
    async () =>
      (await prompt()
        .$(".plugin-prompt-score, .plugin-prompt-subtitle, .plugin-prompt-refused")
        .isExisting()) || (await prompt().$("[role='alert']").isExisting()),
    { timeout: 60_000, timeoutMsg: `the install prompt of ${name} never finished its check` },
  );
}

export async function cancelPrompt(): Promise<void> {
  await prompt().$("button=Cancel").click();
  await prompt().waitForDisplayed({ reverse: true, timeout: 10_000 });
}

/** Installs from an open prompt (ticking the confirmation box when there is one). */
export async function confirmInstall(name: string): Promise<void> {
  const tick = prompt().$(`.//div[${cls("plugin-prompt-confirm")}]//input[@type='checkbox']`);
  if ((await tick.isExisting()) && !(await tick.isSelected())) await tick.click();
  const install = promptInstall();
  await install.waitForClickable({ timeout: 30_000 });
  await install.click();
  await expect($(".plugin-toast")).toHaveText(expect.stringContaining(`${name} installed`), {
    wait: 60_000,
  });
}

/** An Installed row by the plugin's display name. */
export const installedRow = (name: string) =>
  $(
    `//ul[${cls("plugins-installed-list")}]/li[.//span[${cls("plugins-installed-name")}][contains(.,'${name}')]]`,
  );

/** "Don't allow high-risk plugins" in Settings → Plugins → Installed → Plugin safety. */
export async function setBlockHighRisk(on: boolean): Promise<void> {
  await openPlugins("Installed");
  const toggle = $(
    `//div[${cls("settings-switch")}][.//*[starts-with(normalize-space(.),"Don't allow high-risk plugins")]]//input[@role='switch']`,
  );
  await toggle.scrollIntoView();
  if ((await toggle.isSelected()) !== on) await toggle.click();
  await browser.waitUntil(async () => (await toggle.isSelected()) === on, {
    timeout: 10_000,
    timeoutMsg: `the safety setting did not turn ${on ? "on" : "off"}`,
  });
}
