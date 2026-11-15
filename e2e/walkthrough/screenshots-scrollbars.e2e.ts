// Screenshots for issue #875 (scrollbars drawn on top of menus): the channel tree of the load-test
// server (~300 channels) scrolls, and the channel menu opened over it covers the tree's scrollbar,
// in dark and light.
//
// A runs on a display of its own at 2x scale, so the pictures come out at the size of the other
// documentation shots.
//
// Not part of the suite (outside specs/). Start the load-test server first, then run:
//   testenv/scale/up.sh
//   npm run e2e -- --spec e2e/walkthrough/screenshots-scrollbars.e2e.ts
import type { ChildProcess } from "node:child_process";
import { browser, $ } from "@wdio/globals";
import { app } from "../helpers/index.ts";
import { channelName, clickButton, finishWizard, trustIfAsked } from "../helpers/chat-ui.ts";
import { both, HIDPI_ENV, startHidpiDisplay } from "./shots.ts";

/** A script contextmenu event, not a WebDriver right-click (see theme-language.e2e.ts). */
async function openChannelMenu(name: string): Promise<void> {
  await browser.execute((channel) => {
    const label = Array.from(document.querySelectorAll(".tree-channel__name")).find(
      (span) => span.textContent.trim() === channel,
    );
    const row = label?.closest('[role="treeitem"]');
    if (!row) throw new Error(`no tree row for ${channel}`);
    // Opening channels scrolls the tree a little; the picture starts at its top.
    for (let el = row.parentElement; el; el = el.parentElement) {
      if (el.scrollHeight > el.clientHeight && getComputedStyle(el).overflowY !== "visible") {
        el.scrollTop = 0;
        break;
      }
    }
    const box = row.getBoundingClientRect();
    row.dispatchEvent(
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        button: 2,
        clientX: box.left + 8,
        clientY: box.top + 8,
      }),
    );
  }, name);
  await $("//div[@role='menu']").waitForDisplayed();
  // Scrollbars hide when idle; the pointer on the tree's scrollbar keeps it in the picture.
  const thumb = await browser.execute((channel) => {
    const label = Array.from(document.querySelectorAll(".tree-channel__name")).find(
      (span) => span.textContent.trim() === channel,
    );
    let el = label?.parentElement ?? null;
    while (
      el &&
      !(el.scrollHeight > el.clientHeight && getComputedStyle(el).overflowY !== "visible")
    ) {
      el = el.parentElement;
    }
    if (!el) throw new Error("no scrolling tree");
    const box = el.getBoundingClientRect();
    return { x: Math.round(box.right - 2), y: Math.round(box.top + 20) };
  }, name);
  await browser.action("pointer").move(thumb).perform();
}

/** The load-test server of testenv/scale. */
const SCALE_PORT = 64850;

describe("scrollbar screenshots (issue #875)", function () {
  this.timeout(20 * 60_000);
  this.bail(true);

  let xvfb: ChildProcess | undefined;

  before(async function () {
    this.timeout(15 * 60_000);
    xvfb = await startHidpiDisplay();
    await app.restart({ freshProfile: true, env: { ...HIDPI_ENV } });
    await browser.setWindowSize(1440, 900);
    await finishWizard("Alex");
    // Not addFirstServer: that waits for the "Lobby" of the small test servers.
    const address = $("//label[.='Server address or mumble:// link']/following::input[1]");
    await address.waitForDisplayed({ timeout: 30_000 });
    await address.setValue(`localhost:${String(SCALE_PORT)}`);
    await clickButton("Add and connect");
    await trustIfAsked();
    await channelName("Welcome Lobby").waitForDisplayed({ timeout: 30_000 });
    // Top-level channels start collapsed; opening two makes the tree taller than the window.
    await browser.execute(() => {
      for (const name of ["Clans", "Gaming"]) {
        const label = Array.from(document.querySelectorAll(".tree-channel__name")).find(
          (span) => span.textContent.trim() === name,
        );
        const toggle = label?.closest('[role="treeitem"]')?.querySelector(".tree-channel__toggle");
        if (!(toggle instanceof HTMLElement)) throw new Error(`no toggle for ${name}`);
        toggle.click();
      }
    });
    await channelName("Starfall").waitForDisplayed();
  });

  after(() => {
    xvfb?.kill();
  });

  it("channel menu over the scrolling channel tree", async () => {
    await both("scrollbar-channel-menu", () => openChannelMenu("Gold Lions"));
  });
});
