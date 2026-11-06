// Shared steps for specs that use the Settings window (#161): ten groups in a vertical tablist,
// each page a grid of section cards, and the sidebar search with grouped results.
import { $, browser } from "@wdio/globals";

/** Top-level groups in sidebar order, by their English names. */
export const SETTINGS_GROUPS = [
  "General",
  "Account & Identity",
  "Voice & Audio",
  "Chat & Notifications",
  "Privacy & Connections",
  "Plugins",
  "Appearance",
  "Servers",
  "Gaming & Overlay",
  "Advanced & Developer",
  "About",
] as const;
export type SettingsGroup = (typeof SETTINGS_GROUPS)[number];

/**
 * The sidebar tab of `group`, found by its label: badges (the Plugins update count, "New") add
 * their own text to the tab.
 */
export function settingsTab(group: SettingsGroup) {
  return $(
    `//button[@role='tab'][.//span[contains(@class,'settings__tab-label')][normalize-space(.)='${group}']]`,
  );
}

/** Opens Settings through Quick settings → "All settings…" (no-op when it is already open). */
export async function openAllSettings(): Promise<void> {
  if (await $(".settings__nav").isExisting()) return;
  const quick = $("button[aria-label='Quick settings']");
  await quick.waitForClickable({ timeout: 15_000 });
  await quick.click();
  const all = $("//button[@role='menuitem'][contains(., 'All settings')]");
  await all.waitForClickable({ timeout: 10_000 });
  await all.click();
  await $(".settings__nav").waitForExist({ timeout: 15_000 });
}

/** What sits on top of the tab's centre, for the failure message when it cannot be clicked. */
async function describeTabHit(group: SettingsGroup): Promise<string> {
  return browser.execute((label: string) => {
    const tab = [...document.querySelectorAll("button[role='tab']")].find(
      (el) => el.querySelector(".settings__tab-label")?.textContent.trim() === label,
    );
    if (!tab) return "tab missing";
    const r = tab.getBoundingClientRect();
    const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
    const n = (value: number) => String(Math.round(value));
    return `viewport ${n(innerWidth)}x${n(innerHeight)}, tab ${n(r.x)},${n(r.y)} ${n(r.width)}x${n(r.height)}, hit ${hit?.tagName ?? ""}.${hit?.className ?? ""}`;
  }, group);
}

/** Opens Settings (if needed) and selects `group`; waits until its page shows. */
export async function openSettingsGroup(group: SettingsGroup): Promise<void> {
  await openAllSettings();
  const tab = settingsTab(group);
  try {
    await tab.waitForClickable({ timeout: 10_000 });
  } catch (error) {
    throw new Error(`settings tab ${group} not clickable: ${await describeTabHit(group)}`, {
      cause: error,
    });
  }
  await tab.click();
  await browser.waitUntil(async () => (await tab.getAttribute("aria-selected")) === "true", {
    timeout: 10_000,
    timeoutMsg: `settings group ${group} never became selected`,
  });
  await $(
    `//h2[contains(@class,'settings-page__title')][normalize-space(.)='${group}']`,
  ).waitForDisplayed({
    timeout: 10_000,
  });
}

/** A section card on the open settings page, by its title. */
export function settingsCard(title: string) {
  return $(`//section[contains(@class,'settings-group')][@data-title='${title}']`);
}

/** Types `query` into the sidebar search (replacing what was there). */
export async function searchSettings(query: string): Promise<void> {
  const field = $("input.settings-search__input");
  await field.waitForDisplayed({ timeout: 10_000 });
  await field.setValue(query);
  await $(".settings-search__results, .settings-search__empty").waitForExist({ timeout: 10_000 });
}

/**
 * Turns on Developer mode in Advanced & Developer. The fixture plugin registry serves plain http
 * on 127.0.0.1, which the client only accepts in Developer mode.
 */
export async function turnOnDeveloperMode(): Promise<void> {
  await openSettingsGroup("Advanced & Developer");
  const toggle = $(
    "//div[contains(concat(' ',normalize-space(@class),' '),' settings-switch ')][.//*[starts-with(normalize-space(.),'Developer mode')]]//input[@role='switch']",
  );
  await toggle.scrollIntoView();
  if (!(await toggle.isSelected())) await toggle.click();
  await browser.waitUntil(() => toggle.isSelected(), { timeoutMsg: "Developer mode stayed off" });
}
