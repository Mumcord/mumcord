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

/** The sidebar tab of `group`. */
export function settingsTab(group: SettingsGroup) {
  return $(`//button[@role='tab'][normalize-space(.)='${group}']`);
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

/** Opens Settings (if needed) and selects `group`; waits until its page shows. */
export async function openSettingsGroup(group: SettingsGroup): Promise<void> {
  await openAllSettings();
  const tab = settingsTab(group);
  await tab.waitForClickable({ timeout: 10_000 });
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
