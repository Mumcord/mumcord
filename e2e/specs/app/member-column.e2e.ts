// Issue #165 (FR-3.90): the members button in the chat header hides the member column completely
// and shows it again; while hidden it shows the channel's head count and a green dot while someone
// talks, with the talker named in its tooltip. Ctrl+Shift+M toggles it, and the state survives a
// restart. The narrow-window overlay is covered by the vitest suite (no window manager here).
import { join } from "node:path";
import { browser, expect, $ } from "@wdio/globals";
import { app, repoRoot, testenv } from "../../helpers/index.ts";
import { ports } from "../../helpers/testenv.ts";
import {
  addFirstServer,
  finishWizard,
  joinChannel,
  reconnectSaved,
  ScriptBot,
  selectChannel,
} from "../../helpers/chat-ui.ts";

const speech = join(repoRoot, "testenv", "audio", "speech.wav");
const ME = "MemberQA";
const TALKER = "Freya";

const toggle = () => $("button.members-toggle");

/**
 * Hover the members button until its tooltip is open. The tooltip layer only reacts when the
 * pointer *enters* a trigger, and the click in the test before keeps this button's tooltip away
 * until the pointer leaves it, so a plain `moveTo()` can do nothing at all. Moving off and back
 * and retrying also survives a slow CI worker racing the tooltip delay or an unrelated scroll
 * closing it again (issue #216).
 */
async function hoverUntilTooltip(): Promise<string> {
  const detail = $("[role='tooltip'] .tooltip__detail");
  await browser.waitUntil(
    async () => {
      await $(".chat-header__title").moveTo(); // the pointer leaves the button
      await toggle().moveTo(); // ...and enters it again: that schedules the tooltip
      return detail.waitForDisplayed({ timeout: 3_000 }).catch(() => false);
    },
    { timeout: 20_000, timeoutMsg: "tooltip not displayed after repeated hovers" },
  );
  return detail.getText();
}

async function expectShown(shown: boolean): Promise<void> {
  await browser.waitUntil(async () => (await toggle().getAttribute("aria-expanded")) === String(shown), {
    timeoutMsg: `member column not ${shown ? "shown" : "hidden"}`,
  });
  // The inert attribute, not the property, which the driver reports as "false" when absent.
  const inert = await browser.execute(() => document.getElementById("members")?.hasAttribute("inert"));
  expect(inert).toBe(!shown);
}

describe("member column (#165)", () => {
  let freya: ScriptBot;

  before(async () => {
    await testenv.up("v15");
    await app.restart({ freshProfile: true });
    await finishWizard(ME);
    await addFirstServer(ports.v15);
    freya = new ScriptBot({ name: TALKER, timeout: 300 });
    await freya.connected();
    await freya.run("join Lobby");
    // Voice, and with it the talking state, only reaches people in the same channel.
    await joinChannel("Lobby");
    await selectChannel("Lobby");
  });

  after(() => {
    freya.stop();
  });

  it("starts with the column shown and the header button expanded", async () => {
    await toggle().waitForDisplayed();
    expect(await toggle().getAttribute("aria-controls")).toBe("members");
    expect(await toggle().getAttribute("aria-label")).toBe("Hide members");
    await expectShown(true);
  });

  it("hides the column and shows the head count instead", async () => {
    await toggle().click();
    await expectShown(false);
    await browser.waitUntil(async () => /^Show members \(\d+\)$/.test((await toggle().getAttribute("aria-label")) ?? ""), {
      timeoutMsg: "no head count on the members button",
    });
    expect(await $(".members-toggle__count").getText()).toMatch(/^\d+$/);
  });

  it("shows the talking dot and names the talker in the tooltip", async () => {
    // Talk for longer than the audio itself: the hover checks below must not run after the play
    // ended, when the tooltip has no talker to name (issue #216).
    const playing = freya.run(`play ${speech} --loop --seconds 30`, 60_000);
    // The talking state has to be there before hovering: the tooltip only names the talker then.
    await $(".members-toggle__talking").waitForExist({ timeout: 15_000 });
    expect(await hoverUntilTooltip()).toBe(`${TALKER} is talking`);
    await playing;
    await $(".members-toggle__talking").waitForExist({ reverse: true, timeout: 15_000 });
  });

  it("toggles the column with Ctrl+Shift+M", async () => {
    await browser.keys(["Control", "Shift", "m"]);
    await expectShown(true);
    await browser.keys(["Control", "Shift", "m"]);
    await expectShown(false);
  });

  it("remembers the hidden column across a restart", async () => {
    await app.restart();
    await reconnectSaved();
    await selectChannel("Lobby");
    await toggle().waitForDisplayed();
    await expectShown(false);
    await toggle().click();
    await expectShown(true);
  });
});
