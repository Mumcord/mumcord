import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { browser, $, expect } from "@wdio/globals";
import { app, botInBackground, testenv, type BackgroundBot } from "../../helpers/index.ts";
import { run, runOk } from "../../helpers/run.ts";
import { waitConnected } from "../../helpers/chat-ui.ts";
import { typeInto } from "../../helpers/servers-ui.ts";
import { button, finishWizardWithDefaults, waitForText, wizardStep } from "../../helpers/wizard-util.ts";

// Issue #60: the hidden packet inspector. Seven clicks on the version in About unlock it; on
// connecting, the app greets the bot that is already there with a Mumcord hello (plugin data),
// which must show up as a sent row in the inspector's own window, expand to its decoded content
// and be exportable. WebDriver sees that window as a second window handle.
const MY_NAME = "Inspector-Tester";
const BOT_NAME = "Inspector-Bot";

async function openSettings(tab: string): Promise<void> {
  await $("button[aria-label='Quick settings']").click();
  await $("//button[@role='menuitem'][contains(., 'All settings')]").click();
  await $(`//button[@role='tab'][normalize-space()='${tab}']`).click();
}

let mainWindow = "";

/** Switches WebDriver to the inspector window once it exists. */
async function switchToInspector(): Promise<void> {
  let inspector = "";
  await browser.waitUntil(
    async () => {
      const handles = await browser.getWindowHandles();
      inspector = handles.find((h) => h !== mainWindow) ?? "";
      return inspector !== "";
    },
    { timeout: 15_000, timeoutMsg: "packet inspector window never opened" },
  );
  await browser.switchToWindow(inspector);
}

/** Types `path` into the native save dialog titled `title` (WebDriver cannot reach it). */
async function saveVia(title: string, path: string): Promise<void> {
  let dialog = "";
  await browser.waitUntil(
    async () => {
      const windows = await run("xdotool", ["search", "--class", "mumcord"]);
      for (const id of windows.stdout.split("\n").filter((line) => line !== "")) {
        const name = await run("xdotool", ["getwindowname", id]);
        if (name.stdout.startsWith(title)) {
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

describe("packet inspector (issue #60)", () => {
  let peer: BackgroundBot | undefined;

  before(async () => {
    await testenv.reset("v15");
  });

  after(() => {
    peer?.stop();
  });

  it("completes the wizard", async () => {
    await waitForText(wizardStep(1));
    await $("input[type=text]").setValue(MY_NAME);
    await button("Next").click();
    await finishWizardWithDefaults();
  });

  it("is hidden until the version in About is clicked seven times", async () => {
    await openSettings("About");
    expect(await $("//button[@role='tab'][normalize-space()='Packet inspector']").isExisting()).toBe(false);
    const version = $("//dt[normalize-space()='Version']/following-sibling::dd[1]");
    await browser.waitUntil(async () => /\d+\.\d+/.test(await version.getText()), {
      timeoutMsg: "version not loaded",
    });
    for (let i = 0; i < 7; i++) await version.click();
    await waitForText("Developer tools unlocked");
    await $("//button[@role='tab'][normalize-space()='Packet inspector']").waitForDisplayed();
    await browser.keys(["Escape"]);
  });

  it("captures the Mumcord hello sent to a user already on the server", async () => {
    peer = botInBackground(["script"], { name: BOT_NAME, timeout: 60 });
    peer.send("sleep 55");
    // Let the bot log in first, so the app greets it right after its own sync.
    await browser.pause(2500);
    const address = $("input[placeholder='mumble.example.org']");
    await address.waitForDisplayed();
    await typeInto("input[placeholder='mumble.example.org']", `mumble://${MY_NAME}@${testenv.address("v15")}`);
    await button("Add and connect").click();
    await button("Trust and connect").click();
    await waitConnected();

    mainWindow = await browser.getWindowHandle();
    await openSettings("Packet inspector");
    expect(await $("table").isExisting()).toBe(false);
    await button("Open packet inspector").click();
    await switchToInspector();
    await $("//main[h1[normalize-space()='Packet inspector']]").waitForDisplayed();
    const row = $("//tr[contains(@class,'pi-row-sent')][.//code[normalize-space()='mumcord.hello']]");
    await row.waitForDisplayed({ timeout: 15_000, timeoutMsg: "no sent hello row" });
    expect(await row.getText()).toContain("Mumcord hello");
    expect(await row.getText()).toContain(BOT_NAME);
  });

  it("expands the row to the decoded hello and raw hex", async () => {
    const row = $("//tr[contains(@class,'pi-row-sent')][.//code[normalize-space()='mumcord.hello']]");
    const toggle = row.$("button.pi-expand");
    await toggle.click();
    expect(await toggle.getAttribute("aria-expanded")).toBe("true");
    const decoded = $("pre[aria-label='Decoded content']");
    await decoded.waitForDisplayed();
    expect(await decoded.getText()).toContain('"app": "mumcord"');
    await button("Show raw hex").click();
    expect(await $("pre[aria-label='Raw bytes in hex']").getText()).toMatch(/^0000 {2}7b/);
  });

  it("exports the list as JSON", async () => {
    await button("Export JSON").click();
    await saveVia("Export JSON", join(app.profile(), "mumcord-packets.json"));
    const found = (): string | undefined => readdirSync(app.profile()).find((f) => f.startsWith("mumcord-packets"));
    await browser.waitUntil(() => found() !== undefined, { timeout: 10_000, timeoutMsg: "export not written" });
    const text = readFileSync(join(app.profile(), found() ?? ""), "utf8");
    const exported = JSON.parse(text) as { warning: string; packets: { kind: string; direction: string }[] };
    expect(exported.warning).toMatch(/personal data/);
    expect(exported.packets.some((p) => p.kind === "mumcord.hello" && p.direction === "sent")).toBe(true);
  });

  it("keeps a single window: opening again brings the open one back", async () => {
    await browser.switchToWindow(mainWindow);
    await button("Open packet inspector").click();
    await browser.pause(1000);
    expect(await browser.getWindowHandles()).toHaveLength(2);
  });

  it("ignores Escape and closes on Ctrl+W, leaving the app running", async () => {
    await switchToInspector();
    await browser.keys(["Escape"]);
    await browser.pause(500);
    expect(await browser.getWindowHandles()).toHaveLength(2);
    // The window is gone before the keys are released, so WebDriver reports the release as
    // failed ("no such window"); the window count below is what matters.
    await browser.keys(["Control", "w"]).catch((error: unknown) => {
      console.log(`[e2e] Ctrl+W: ${String(error)}`);
    });
    await browser.waitUntil(async () => (await browser.getWindowHandles()).length === 1, {
      timeout: 10_000,
      timeoutMsg: "Ctrl+W did not close the packet inspector",
    });
    await browser.switchToWindow(mainWindow);
    await button("Open packet inspector").waitForDisplayed();
  });
});
