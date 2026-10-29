import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { browser, $, expect } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import { run, runOk } from "../../helpers/run.ts";
import { waitConnected } from "../../helpers/chat-ui.ts";
import { typeInto } from "../../helpers/servers-ui.ts";
import { button, finishWizardWithDefaults, waitForText, wizardStep } from "../../helpers/wizard-util.ts";

// AC-1.28 (diagnostic file contents/privacy) and AC-1.29 (Settings -> About licenses), driven
// through the real UI. The test server gets a server password; the bot sends a unique chat text.
const SERVER_PASSWORD = "SrvPw-Zq7731-unique";
const CHAT_TEXT = "ChatCanary-Xk9921-unique";
const MY_NAME = "Qa-Tester-Wombat";
const BOT_NAME = "Qa-Bot-Pelican";

async function dumpDiagnostics(path: string): Promise<void> {
  // The native save dialog cannot be driven by WebDriver: type the path into the GTK dialog.
  await $("button=Save diagnostic file…").click();
  // The dialog opens asynchronously and Xvfb has no window manager: keys sent before it is
  // mapped and focused are lost (same flake as the wizard's file chooser), so wait and focus it.
  // xdotool's own name search skips titles with a non-ASCII character such as the "…" of this
  // dialog's title, so find the app's windows by class and compare their names ourselves.
  let dialog = "";
  await browser.waitUntil(
    async () => {
      const windows = await run("xdotool", ["search", "--class", "mumcord"]);
      for (const id of windows.stdout.split("\n").filter((line) => line !== "")) {
        const name = await run("xdotool", ["getwindowname", id]);
        if (name.stdout.startsWith("Save diagnostic file")) {
          dialog = id;
          return true;
        }
      }
      return false;
    },
    { timeout: 20_000, interval: 250, timeoutMsg: "native save dialog never opened" },
  );
  await runOk("xdotool", ["windowfocus", "--sync", dialog]);
  await runOk("xdotool", ["key", "ctrl+l"]);
  await browser.pause(300);
  await runOk("xdotool", ["type", "--delay", "30", path]);
  await browser.pause(300);
  await runOk("xdotool", ["key", "Return"]);
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
    await waitForText(wizardStep(1));
    await $("input[type=text]").setValue(MY_NAME);
    await button("Next").click();
    await finishWizardWithDefaults();
  });

  it("connects with the server password and receives chat", async () => {
    const address = $("input[placeholder='mumble.example.org']");
    await address.waitForDisplayed();
    await typeInto("input[placeholder='mumble.example.org']", `mumble://${MY_NAME}:${SERVER_PASSWORD}@${testenv.address("v15")}`);
    await button("Add and connect").click();
    await button("Trust and connect").click();
    await waitConnected();
    await bot(["say", `<p>${CHAT_TEXT}</p>`], { name: BOT_NAME, timeout: 15, hold: 3 });
    await browser.waitUntil(async () => (await $("#root").getText()).includes(CHAT_TEXT), {
      timeoutMsg: "chat text not shown",
    });
  });

  it("AC-1.28: saved diagnostic file has the facts and no private data", async () => {
    await $("button[aria-label='Quick settings']").click();
    await $("//button[@role='menuitem'][contains(., 'All settings')]").click();
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

  it("AC-1.29: About lists AGPL license and Mumble's BSD notice", async () => {
    await browser.pause(500);
    await $("//button[@role='tab'][normalize-space()='About']").click();
    const license = $("pre[aria-label='Mumcord license (AGPL-3.0)']");
    await license.waitForDisplayed();
    expect(await license.getText()).toMatch(/GNU AFFERO GENERAL PUBLIC LICENSE/);

    // Issue #138: the third-party licences open in a window of their own, a second WebDriver
    // window handle.
    const mainWindow = await browser.getWindowHandle();
    await button("Show third-party licences").click();
    let licences = "";
    await browser.waitUntil(
      async () => {
        licences = (await browser.getWindowHandles()).find((h) => h !== mainWindow) ?? "";
        return licences !== "";
      },
      { timeout: 15_000, timeoutMsg: "licences window never opened" },
    );
    await browser.switchToWindow(licences);
    await $("h1=Third-party licences").waitForDisplayed();
    expect(await $("main").getText()).toContain("AGPL-3.0-only");
    await $("input[type=search]").setValue("Mumble");
    const mumble = $("//button[@aria-expanded][contains(., 'Mumble protocol and plugin interface')]");
    await mumble.click();
    expect(await mumble.getAttribute("aria-expanded")).toBe("true");
    const text = await $("pre[aria-label='Licence text of Mumble protocol and plugin interface']").getText();
    // Mumble's own BSD notice, not just any BSD-licensed crate.
    expect(text).toMatch(/Copyright \(C\) 2005-\d{4} The Mumble Developers/);
    expect(text).toContain("Neither the name of The Mumble Developers");

    // The window is gone before the keys are released, so WebDriver reports the release as
    // failed ("no such window"); the window count below is what matters.
    await browser.keys(["Control", "w"]).catch((error: unknown) => {
      console.log(`[e2e] Ctrl+W: ${String(error)}`);
    });
    await browser.waitUntil(async () => (await browser.getWindowHandles()).length === 1, {
      timeout: 10_000,
      timeoutMsg: "Ctrl+W did not close the licences window",
    });
    await browser.switchToWindow(mainWindow);
    await button("Show third-party licences").waitForDisplayed();
  });
});
