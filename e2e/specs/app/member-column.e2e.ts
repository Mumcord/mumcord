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
    const playing = freya.run(`play ${speech}`, 60_000);
    await $(".members-toggle__talking").waitForExist({ timeout: 15_000 });
    await toggle().moveTo();
    const tip = $("[role='tooltip'] .tooltip__detail");
    await tip.waitForDisplayed({ timeout: 5_000 });
    expect(await tip.getText()).toBe(`${TALKER} is talking`);
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
