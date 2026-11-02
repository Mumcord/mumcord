// Spec 06 C7 on a Mumble 1.3 server (no plugin data), with fixtures from the fixture registry:
// - AC-6.17: plugins still run; the strip says the shared parts are unavailable.
// - AC-6.15: fx-notify-flood calls `notify` five times at start. In Do Not Disturb no entry
//   appears; afterwards one start gives exactly one entry.
// - AC-6.16: fx-webview-probe's sealed page tries fetch, parent/top access, storage and the Tauri
//   IPC. Nothing may be reachable, its probe request must never reach the local listener that
//   stands in for a proxy log, and the message bridge must still work both ways.
import { createServer, type Server } from "node:http";
import { browser, expect, $, $$ } from "@wdio/globals";
import { app, registry, testenv } from "../../helpers/index.ts";
import { addFirstServer, finishWizard } from "../../helpers/chat-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";

/** The port fx-webview-probe's page probes by default (crates/plugins/tests/fixtures/safety). */
const PROBE_PORT = 18999;
const UNSUPPORTED = "This server doesn't support shared plugin features";

function dndRow() {
  return $("//button[@role='menuitemcheckbox'][starts-with(normalize-space(.), 'Do Not Disturb')]");
}

async function setDnd(on: boolean): Promise<void> {
  const quick = $("button[aria-label='Quick settings']");
  await quick.waitForClickable();
  await quick.click();
  await dndRow().waitForDisplayed();
  if ((await dndRow().getAttribute("aria-checked")) !== String(on)) await dndRow().click();
  await expect(dndRow()).toHaveAttribute("aria-checked", String(on));
  await browser.keys("Escape");
}

async function openPluginsTab(tab: "Browse" | "Installed"): Promise<void> {
  await openSettingsGroup("Plugins");
  const button = $(`//div[contains(@class,'plugins-tabs')]//button[@role='tab'][starts-with(normalize-space(.), '${tab}')]`);
  await button.waitForClickable({ timeout: 10_000 });
  await button.click();
}

async function install(name: string): Promise<void> {
  await openPluginsTab("Browse");
  const button = $(`button[aria-label='Install ${name}']`);
  await button.waitForClickable({ timeout: 30_000 });
  await button.click();
  // The prompt checks the package first (progress bar), then offers Install.
  const confirm = $(
    "//div[contains(concat(' ',@class,' '),' plugin-prompt ')]//button[normalize-space(.)='Install']",
  );
  await confirm.waitForClickable({ timeout: 60_000 });
  await confirm.click();
  await expect($(".plugin-toast")).toHaveText(expect.stringContaining(`${name} installed`), {
    wait: 60_000,
  });
}

async function closeSettings(): Promise<void> {
  await browser.keys("Escape");
  await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
}

/** Notification centre entries of the notify fixture (title "Fixture", body "Ping"). */
async function pingEntries(): Promise<number> {
  const centre = $("[data-testid='notification-centre']");
  if (!(await centre.isExisting())) {
    await $("[data-testid='notification-bell']").click();
    await centre.waitForDisplayed();
  }
  const entries = await $$(
    "//section[@data-testid='notification-centre']//*[@data-testid='centre-entry'][contains(., 'Ping')]",
  );
  const count = entries.length;
  await browser.keys("Escape");
  return count;
}

describe("plugins: notify, sealed web view and an old server (AC-6.15–6.17)", () => {
  let fixtures: registry.FixtureRegistry;
  let listener: Server;
  const probed: string[] = [];

  before(async function () {
    this.timeout(20 * 60_000);
    listener = createServer((req, res) => {
      probed.push(`${req.method ?? "?"} ${req.url ?? "?"}`);
      res.end("reachable");
    });
    await new Promise<void>((resolve, reject) => {
      listener.once("error", reject);
      listener.listen(PROBE_PORT, "127.0.0.1", resolve);
    });
    fixtures = await registry.start();
    await fixtures.publish("fx-notify-flood");
    await testenv.up("v13");
    await app.restart({ env: fixtures.env() });
    await finishWizard();
    await addFirstServer(testenv.ports.v13);
    // The fixture registry serves plain http on 127.0.0.1, which the client only accepts in
    // Developer mode (FR-6.74).
    await openSettingsGroup("Advanced & Developer");
    const developerMode = $("//input[@role='switch'][@id=//label[.='Developer mode']/@for]");
    await developerMode.waitForExist({ timeout: 10_000 });
    await developerMode.click();
    await expect(developerMode).toBeSelected();
    await closeSettings();
  });

  after(async () => {
    await fixtures.stop();
    await new Promise((resolve) => listener.close(resolve));
  });

  it("drops plugin notifications in Do Not Disturb and shows one of five otherwise", async () => {
    await setDnd(true);
    await install("Fixture Notify Flood");
    await closeSettings();
    // The fixture notifies inside `start`, which ran before the install toast.
    expect(await pingEntries()).toBe(0);

    await setDnd(false);
    await openPluginsTab("Installed");
    const toggle = $("input[role='switch'][aria-label='Fixture Notify Flood on']");
    await toggle.click();
    await expect(toggle).not.toBeChecked();
    await toggle.click();
    await expect(toggle).toBeChecked();
    await closeSettings();
    await browser.waitUntil(async () => (await pingEntries()) > 0, {
      timeout: 10_000,
      timeoutMsg: "the plugin notification never reached the centre",
    });
    expect(await pingEntries()).toBe(1);
  });

  it("runs plugins on a 1.3 server and greys the shared parts", async () => {
    await install("Fixture Web View");
    await closeSettings();
    const icon = $("//div[contains(@class,'pstrip-icons')]//button[@aria-label='Fixture Web View']");
    await icon.waitForClickable({ timeout: 15_000 });
    await icon.click();
    await expect($(".pstrip-panel .pstrip-hint")).toHaveText(UNSUPPORTED);
    await expect($(".pstrip-panel")).toHaveAttribute("data-shared", "off");
  });

  it("keeps the web view sealed while its bridge works", async () => {
    await $("button[aria-label='Expand Fixture Web View']").click();
    const frame = $("[role='dialog'][aria-label='Fixture Web View'] iframe.plugin-webview__frame");
    await frame.waitForExist({ timeout: 15_000 });
    await browser.switchFrame(frame);
    try {
      const out = $("#out");
      await browser.waitUntil(async () => (await out.getText()) !== "probing", {
        timeout: 15_000,
        timeoutMsg: "the probe page never reported",
      });
      const results = JSON.parse(await out.getText()) as Record<string, string>;
      // WebKitGTK exposes the raw `ipc` message handler in every frame, so a post does not
      // throw there. Tauri drops every message without the per-launch invoke key, which the
      // sealed frame cannot read (no `__TAURI_INTERNALS__`, no parent access): that probe is
      // the only one allowed to report "reachable".
      const reachable = Object.entries(results)
        .filter(([probe, result]) => result === "reachable" && probe !== "webkit-ipc-post")
        .map(([probe]) => probe);
      expect(reachable).toEqual([]);
      expect(results.fetch).toMatch(/^blocked/);
      expect(results["tauri-internals"]).toBe("undefined");
      await expect($("#echo")).toHaveText("echo received", { wait: 10_000 });
    } finally {
      await browser.switchFrame(null);
    }
    expect(probed).toEqual([]);
  });
});
