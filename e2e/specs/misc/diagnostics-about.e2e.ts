import { spawn } from "node:child_process";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { browser, $, expect } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import { waitConnected } from "../../helpers/chat-ui.ts";
import { typeInto } from "../../helpers/servers-ui.ts";
import { button, finishWizardWithDefaults, waitForText } from "../../helpers/wizard-util.ts";

// AC-1.28 (diagnostic file contents/privacy) and AC-1.29 (Settings -> About licenses), driven
// through the real UI. The test server gets a server password; the bot sends a unique chat text.
const SERVER_PASSWORD = "SrvPw-Zq7731-unique";
const CHAT_TEXT = "ChatCanary-Xk9921-unique";
const MY_NAME = "Qa-Tester-Wombat";
const BOT_NAME = "Qa-Bot-Pelican";

async function dumpDiagnostics(path: string): Promise<void> {
  // The native save dialog cannot be driven by WebDriver: type the path into the GTK dialog.
  await $("button=Save diagnostic file…").click();
  await browser.pause(2500);
  await new Promise<void>((resolve) => {
    const p = spawn("xdotool", ["key", "ctrl+l"], { env: process.env });
    p.on("close", () => {
      resolve();
    });
  });
  await browser.pause(400);
  await new Promise<void>((resolve) => {
    const p = spawn("xdotool", ["type", "--delay", "30", path], { env: process.env });
    p.on("close", () => {
      resolve();
    });
  });
  await new Promise<void>((resolve) => {
    const p = spawn("xdotool", ["key", "Return"], { env: process.env });
    p.on("close", () => {
      resolve();
    });
  });
}

describe("diagnostics and about (AC-1.28, AC-1.29)", () => {
  let file = "";

  before(async () => {
    process.env.SERVER_PASSWORD = SERVER_PASSWORD;
    await testenv.reset("v15");
  });

  after(async () => {
    delete process.env.SERVER_PASSWORD;
    await testenv.reset("v15");
  });

  it("completes the wizard", async () => {
    await waitForText("Step 1 of 3");
    await $("input[type=text]").setValue(MY_NAME);
    await button("Next").click();
    await finishWizardWithDefaults();
  });

  it("connects with the server password and receives chat", async () => {
    const address = $("input[placeholder='mumble.example.org']");
    await address.waitForDisplayed();
    await typeInto("input[placeholder='mumble.example.org']", `mumble://${MY_NAME}:${SERVER_PASSWORD}@localhost:64738`);
    await button("Add and connect").click();
    await button("Trust and connect").click();
    await waitConnected();
    await bot(["say", `<p>${CHAT_TEXT}</p>`], { name: BOT_NAME, timeout: 15, hold: 3 });
    await browser.waitUntil(async () => (await $("#root").getText()).includes(CHAT_TEXT), {
      timeoutMsg: "chat text not shown",
    });
  });

  it("AC-1.28: saved diagnostic file has the facts and no private data", async () => {
    await $("button[aria-label='Settings']").click();
    await $("//button[@role='tab'][normalize-space()='Diagnostics']").click();
    // The GTK save dialog appends its ".txt" filter extension even to a typed ".txt" name.
    file = join(app.profile(), "mumcord-diagnostics.txt.txt");
    await dumpDiagnostics(file);
    const found = (): string | undefined => readdirSync(app.profile()).find((f) => f.startsWith("mumcord-diagnostics"));
    await browser.waitUntil(() => found() !== undefined, {
      timeoutMsg: `file not written; profile has: ${readdirSync(app.profile()).join(",")}`,
      timeout: 10_000,
    });
    file = join(app.profile(), found() ?? "");
    expect(statSync(file).size).toBeGreaterThan(50);
    const text = readFileSync(file, "utf8");
    expect(text).toMatch(/version/i);
    expect(text).toMatch(/os|linux|ubuntu/i);
    expect(text).toMatch(/device/i);
    expect(text).toMatch(/connect|ping|packets/i);
    for (const secret of [CHAT_TEXT, SERVER_PASSWORD, MY_NAME, BOT_NAME]) {
      expect(text).not.toContain(secret);
    }
    expect(text).not.toMatch(/BEGIN [A-Z ]*(CERTIFICATE|PRIVATE KEY)/);
    // Also scan: no other file with PEM is expected here; nothing else to check.
  });

  it("AC-1.29: About lists MIT license and Mumble's BSD notice", async () => {
    await browser.pause(500);
    await $("//button[@role='tab'][normalize-space()='About']").click();
    const license = $("pre[aria-label='Mumcord license (MIT)']");
    await license.waitForDisplayed();
    expect(await license.getText()).toMatch(/MIT License/i);
    const notices = $("pre[aria-label='Third-party notices']");
    await browser.waitUntil(async () => !(await notices.getText()).startsWith("Loading"), {
      timeoutMsg: "notices not loaded",
    });
    const text = await notices.getText();
    expect(text).toMatch(/Mumble/);
    expect(text).toMatch(/BSD|Redistribution and use in source and binary forms/i);
    // Mumble's own BSD notice, not just any BSD-licensed crate.
    expect(text).toMatch(/Copyright \(C\) 2005-\d{4} The Mumble Developers/);
    expect(text).toContain("Neither the name of The Mumble Developers");
  });
});
