// Issue #163 (FR-3.127): the sharer's control bar at the top of the chat. One client only —
// mumbot cannot be a watch-party participant or a viewer, so the second side of these features is
// covered by the engine live tests (`crates/engine/tests/live_watch_party.rs`,
// `live_screen_share.rs`), not here. What this spec checks is what one client can show on its own:
// the bar appears only while sharing, says what was picked and how long it runs, pause and resume
// work, and the view stays usable as the mini bar after leaving the picture.
//
// Run: npm run e2e -- --spec e2e/specs/media/share-bar.e2e.ts
import { browser, $, expect } from "@wdio/globals";
import { app, testenv } from "../../helpers/index.ts";
import { ports } from "../../helpers/testenv.ts";
import { addFirstServer, finishWizard, joinChannel } from "../../helpers/chat-ui.ts";
import { button } from "../../helpers/wizard-util.ts";

const ME = "ShareBarQA";

/** The control bar at the top of the chat (issue #163). */
const bar = () => $("//div[contains(@class,'sharer-bar')]");

/** Waits until `el` (a container) contains `text`. */
async function waitText(el: WebdriverIO.Element, text: string): Promise<void> {
  await browser.waitUntil(async () => (await el.getText()).includes(text), {
    timeout: 20_000,
    timeoutMsg: `the bar did not show "${text}"`,
  });
}

async function openShareTab(): Promise<void> {
  await $("button[aria-label='Quick settings']").click();
  await $("//button[@role='menuitem'][contains(., 'All settings')]").click();
  await $("//button[@role='tab'][normalize-space()='Screen share']").click();
}

describe("sharer control bar (#163)", () => {
  before(async () => {
    await testenv.up("v15");
    await app.restart({ freshProfile: true });
    await finishWizard(ME);
    await addFirstServer(ports.v15);
    await joinChannel("Lobby");
    await openShareTab();
  });

  it("shows nothing before a share starts", async () => {
    expect(await bar().isExisting()).toBe(false);
  });

  it("shows the bar with the source and the running time while sharing", async () => {
    const start = await button("Share screen");
    if (!(await start.isExisting())) {
      // The container has no desktop session for X11 capture; the bar cannot be tested without a
      // share, so the spec reports that instead of failing silently.
      throw new Error("no share button: the container has no desktop session for X11 capture");
    }
    await start.click();
    const shown = await bar();
    await waitText(shown, "Live");
    await expect(shown).toBeExisting();
    const text = await shown.getText();
    expect(text).toContain("min");

    // Pause and resume go through the engine, so the bar must show the change.
    await $("//button[contains(., 'Pause')]").click();
    await waitText(shown, "Paused");
    await $("//button[contains(., 'Resume')]").click();
  });

  it("keeps working as the mini bar after the picture is left", async () => {
    await $("button[aria-label='Leave the picture']").click();
    const mini = $("//div[contains(@class,'mini-bar')]");
    await expect(mini).toBeExisting();
    // Z opens the picture again, and the keyboard stays usable on the bar.
    await browser.keys(["z"]);
    await expect($("//div[contains(@class,'media-frame')]")).toBeExisting();
    await browser.keys(["Escape"]);
    await expect(mini).toBeExisting();
  });

  after(async () => {
    const stop = await button("Stop sharing");
    if (await stop.isExisting()) {
      await stop.click();
    }
    const leave = await button("Leave this server");
    if (await leave.isExisting()) {
      await leave.click();
    }
  });
});
