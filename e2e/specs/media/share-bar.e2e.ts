// Issue #163 (FR-3.125–FR-3.127) with one client only: the sharer's control bar at the top of the
// chat, and the picture view it belongs to. mumbot cannot be a share viewer (no plugin data), so
// the second side of these features is covered by the engine live tests
// (`crates/engine/tests/live_screen_share.rs`), not here. What this spec checks is what one client
// can show on its own: the bar appears only while sharing, says what was picked and how long the
// share runs, pause and resume reach the engine, the mini bar is what remains after leaving the
// picture, and Stop ends the share.
//
// Run: npm run e2e -- --spec e2e/specs/media/share-bar.e2e.ts
import { browser, $, expect } from "@wdio/globals";
import { app, testenv } from "../../helpers/index.ts";
import { ports } from "../../helpers/testenv.ts";
import { addFirstServer, finishWizard, joinChannel } from "../../helpers/chat-ui.ts";
import { button } from "../../helpers/wizard-util.ts";

const ME = "ShareBarQA";

/** The control bar at the top of the chat (issue #163), named like the accessibility tests expect. */
const bar = () => $("//div[contains(@class,'sharer-bar')]");

/** Waits until `el` contains `text`. */
async function waitText(el: WebdriverIO.Element, text: string): Promise<void> {
  await browser.waitUntil(async () => (await el.getText()).includes(text), {
    timeout: 20_000,
    timeoutMsg: `the share bar did not show "${text}"`,
  });
}

describe("sharer control bar (#163)", () => {
  before(async () => {
    await testenv.up("v15");
    await app.restart({ freshProfile: true });
    await finishWizard(ME);
    await addFirstServer(ports.v15);
    await joinChannel("Lobby");
  });

  it("shows nothing before the share starts", async () => {
    expect(await bar().isExisting()).toBe(false);
  });

  it("shows the source and the running time while sharing", async () => {
    const start = await button("Share screen");
    if (!(await start.isExisting())) {
      throw new Error(`no share button (reason: ${(await $("//div[contains(@class,'share-panel')]")).getText()})`);
    }
    await start.click();
    const shown = await bar();
    await browser.waitUntil(() => shown.isExisting(), { timeout: 20_000, timeoutMsg: "the bar did not appear" });
    await waitText(shown, "Live");
    // A share that just started shows as one minute running, with what was picked.
    await waitText(shown, "min");

    await $("//button[contains(., 'Pause')]").click();
    await waitText(shown, "Paused");
    await $("//button[contains(., 'Resume')]").click();
  });

  it("stops the share", async () => {
    await button("Stop sharing").click();
    await browser.waitUntil(async () => !(await bar().isExisting()), {
      timeout: 20_000,
      timeoutMsg: "the share did not stop",
    });
  });
});
