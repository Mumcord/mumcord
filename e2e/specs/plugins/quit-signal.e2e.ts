// #748: when the system ends Mumcord (SIGTERM at logout, shutdown or `kill`), the app quits like
// "Quit" does, so plugins get their `stop()`. The test plugin `test-quit-save` writes a value to
// its storage in `stop()` and nowhere else; after SIGTERM and a restart the value is there, and
// the run does not count as a crash.
import { existsSync } from "node:fs";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { app, registry } from "../../helpers/index.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { completeWizard } from "../../helpers/voice-ui.ts";

const ID = "test-quit-save";
const NAME = "Fixture Quit Save";

interface Overview {
  plugins: { id: string; status: { kind: string } }[];
}

interface DataExport {
  entries: { scope: string; key: string; text?: string }[];
}

/** What the plugin has in its storage ("Export data"). */
async function stored(): Promise<DataExport["entries"]> {
  const json = await app.invoke<string>("plugin_export_data", { id: ID });
  return (JSON.parse(json) as DataExport).entries;
}

async function waitRunning(): Promise<void> {
  await browser.waitUntil(
    async () => {
      const { plugins } = await app.invoke<Overview>("plugins_list");
      return plugins.find((p) => p.id === ID)?.status.kind === "running";
    },
    { timeout: 30_000, interval: 250, timeoutMsg: `${NAME} is not running` },
  );
}

describe("plugins: a system quit signal stops the plugins (#748)", () => {
  let fixtures: registry.FixtureRegistry;

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await registry.start();
  });

  after(async () => {
    await fixtures.stop();
  });

  it("stop() runs on SIGTERM: what the plugin saves there is kept", async () => {
    await app.restart({ env: fixtures.env() });
    await completeWizard("Alice");

    // The fixture registry serves plain http on 127.0.0.1, which the client only accepts in
    // Developer mode (FR-6.74).
    await openSettingsGroup("Advanced & Developer");
    const developerMode = $("//input[@role='switch'][@id=//label[.='Developer mode']/@for]");
    await developerMode.waitForExist({ timeout: 10_000 });
    await developerMode.click();
    await expect(developerMode).toBeSelected();

    await openSettingsGroup("Plugins");
    await $("//button[@role='tab'][contains(@class,'plugins-tab')][normalize-space(.)='Browse']").click();
    const install = $(`button[aria-label='Install ${NAME}']`);
    await install.waitForExist({ timeout: 30_000 });
    await install.scrollIntoView({ block: "center" });
    await install.waitForClickable({ timeout: 30_000 });
    await install.click();
    const confirm = $(
      "//div[contains(concat(' ',@class,' '),' plugin-prompt ')]//button[normalize-space(.)='Install']",
    );
    await confirm.waitForClickable({ timeout: 60_000 });
    await confirm.click();
    await expect($(".plugin-toast")).toHaveText(expect.stringContaining(`${NAME} installed`), {
      wait: 60_000,
    });
    await waitRunning();
    // Only `stop()` writes the value, so nothing is stored while the plugin runs.
    expect(await stored()).toEqual([]);

    // The harness ends the app through the driver, which kills it: that start-up restart is on
    // the list already. The quit below must not add a report.
    const crashesBefore = await app.invoke<unknown[]>("crash_list");

    // The marker of a running app; a clean quit removes it, a crash leaves it for the next start.
    const runMarker = join(app.profile(), "data", "app.mumcord.client", "crash-reports", "running");
    expect(existsSync(runMarker)).toBe(true);

    // The signal goes to the app alone, as at logout or shutdown; the driver is left running.
    const pid = app.appPid();
    process.kill(pid, "SIGTERM");
    // The app must end by itself: stopping the plugins has a budget of 2 s.
    await browser.waitUntil(() => !app.alive(pid), {
      timeout: 10_000,
      interval: 100,
      timeoutMsg: "the app did not exit after SIGTERM",
    });
    expect(existsSync(runMarker)).toBe(false);

    await app.restart();
    await waitRunning();
    expect(await stored()).toEqual([{ scope: "global", key: "stopped", text: "saved in stop" }]);
    expect(await app.invoke<unknown[]>("crash_list")).toEqual(crashesBefore);
  });
});
