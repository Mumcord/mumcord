// AC-3.59 (#64): assign a global hotkey to mute; with Mumcord in the background pressing it mutes
// and unmutes; assigning the same key to another action shows a conflict. X11 path (the CI
// session), keys sent with xdotool while a helper window has the focus.
// Issue #584: the defaults sit under the key fields and work while Mumcord has the focus.
import { spawn, type ChildProcess } from "node:child_process";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { app, repoRoot, testenv } from "../../helpers/index.ts";
import { run } from "../../helpers/run.ts";
import { addAndConnect, completeWizard, joinIfReadOnly } from "../../helpers/voice-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";

const ME = "HotkeyQA";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const xdo = async (...args: string[]) => (await run("xdotool", args)).stdout.trim();

/** The key field of a row in Settings → Hotkeys. */
const row = (action: string) =>
  `//div[contains(@class,'hotkey-row')][.//span[normalize-space(.)='${action}']]`;
const hotkeyRow = (action: string) => $(row(action));
const keyField = (action: string) => $(`${row(action)}//button[contains(@class,'hotkey-row__field')]`);
const rowLink = (action: string, label: string) => $(`${row(action)}//button[normalize-space(.)='${label}']`);
const muteButton = () => $("button[aria-label='Mute microphone']");
const deafenButton = () => $("button[aria-label='Deafen']");

describe("global hotkeys (AC-3.59, #64)", () => {
  before(async () => {
    await testenv.up("v15");
    await app.restart();
    await completeWizard(ME);
    await addAndConnect();
    await joinIfReadOnly();
  });

  it("runs the defaults while the Mumcord window has the focus (#584)", async () => {
    await expect(deafenButton()).toHaveAttribute("aria-pressed", "false");
    await browser.keys(["Control", "Shift", "d"]);
    await expect(deafenButton()).toHaveAttribute("aria-pressed", "true");
    await browser.keys(["Control", "Shift", "d"]);
    await expect(deafenButton()).toHaveAttribute("aria-pressed", "false");
  });

  it("shows the defaults under the rows and assigns a key to mute", async () => {
    await openSettingsGroup("Voice & Audio");
    // Nothing is stored yet: the fields show their defaults (issue #584).
    expect(await keyField("Push-to-talk").getText()).toContain("Not set");
    expect(await hotkeyRow("Push-to-talk").getText()).toContain("Default: none");
    expect(await keyField("Mute / unmute microphone").getText()).not.toContain("Not set");
    expect(await keyField("Mute / unmute microphone").getText()).toContain("Shift");
    expect(await keyField("Toggle Do Not Disturb").getText()).toContain("Not set");

    await keyField("Mute / unmute microphone").click();
    await browser.keys("F7");
    await browser.waitUntil(
      async () => (await keyField("Mute / unmute microphone").getText()).includes("F7"),
      { timeout: 10_000, timeoutMsg: "mute hotkey not recorded" },
    );

    // The own key replaces the default, and "Use default" hands the action back to it.
    await rowLink("Mute / unmute microphone", "Use default").click();
    await browser.waitUntil(
      async () => !(await keyField("Mute / unmute microphone").getText()).includes("F7"),
      { timeout: 10_000, timeoutMsg: "the own key did not go away" },
    );
    await keyField("Mute / unmute microphone").click();
    await browser.keys("F7");
    await browser.waitUntil(
      async () => (await keyField("Mute / unmute microphone").getText()).includes("F7"),
      { timeout: 10_000, timeoutMsg: "mute hotkey not recorded the second time" },
    );
  });

  it("refuses the same key a second time as a conflict", async () => {
    await keyField("Deafen / undeafen").click();
    await browser.keys("F7");
    const conflict = $("//*[@role='alert'][contains(., 'Conflict.')]");
    await conflict.waitForDisplayed({ timeout: 10_000 });
    expect(await conflict.getText()).toContain('already used by "Mute / unmute microphone"');
    await conflict.$("button=Cancel").click();
    // Cancelled: the row keeps showing its own default instead of the refused key.
    expect(await keyField("Deafen / undeafen").getText()).not.toContain("F7");
    await browser.keys("Escape");
  });

  it("mutes and unmutes with the key while another window has the focus", async () => {
    await expect(muteButton()).toHaveAttribute("aria-pressed", "false");
    const before = (await xdo("search", "--onlyvisible", "--name", ".")).split("\n");
    const viewer: ChildProcess = spawn(
      "gcr-viewer-gtk4",
      [join(repoRoot, "testenv/certs/A/cert.pem")],
      { stdio: "ignore" },
    );
    try {
      let helper = "";
      for (let i = 0; i < 50 && !helper; i++) {
        await sleep(200);
        const now = (await xdo("search", "--onlyvisible", "--name", ".")).split("\n");
        helper = now.find((w) => w && !before.includes(w)) ?? "";
      }
      if (!helper) throw new Error("helper window did not appear");
      await xdo("windowfocus", "--sync", helper);

      await xdo("key", "F7");
      await browser.waitUntil(async () => (await muteButton().getAttribute("aria-pressed")) === "true", {
        timeout: 10_000,
        timeoutMsg: "the hotkey did not mute while Mumcord was in the background",
      });
      await xdo("key", "F7");
      await browser.waitUntil(async () => (await muteButton().getAttribute("aria-pressed")) === "false", {
        timeout: 10_000,
        timeoutMsg: "the hotkey did not unmute again",
      });
    } finally {
      viewer.kill();
    }
  });
});
