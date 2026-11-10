import { $, browser } from "@wdio/globals";

/**
 * Opens the Members tab of the members column. The column opens on Activity (issue #709), so
 * specs that need the member list or the friends list below it switch first.
 */
export async function openMembersTab(): Promise<void> {
  const tab = $(
    "//div[contains(@class,'activity-tabs__row')]//button[@role='tab'][normalize-space()='Members']",
  );
  await tab.waitForClickable({ timeout: 15_000 });
  if ((await tab.getAttribute("aria-selected")) === "true") return;
  await tab.click();
  await browser.waitUntil(async () => (await tab.getAttribute("aria-selected")) === "true", {
    timeoutMsg: "the Members tab never opened",
  });
}
