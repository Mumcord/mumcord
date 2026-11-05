// AC-9.34 and AC-9.35 (spec 09 §4.12): the one confirmation box of the install prompt and the
// "Don't allow high-risk plugins" setting, against the fixture registries of helpers/pluginenv.ts.
//
// - test-growth (its listing is 1.1.0: web page, relay and two recently added permissions) is
//   High risk (80) and the first plugin installed from the third-party repository, so High risk,
//   own code with websites and the first-install repository warning all apply: one box, one tick.
// - test-score-high, unpacked into a folder and loaded in Developer mode, is the developer plugin
//   (scored for information only, F fixed at 1.00: 83, High).
// - Fixture Strip is the official plugin.
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, expect } from "@wdio/globals";
import { app, pluginenv } from "../../helpers/index.ts";
import { runOk } from "../../helpers/run.ts";
import {
  addRepository,
  cancelPrompt,
  card,
  chipLevel,
  chipText,
  closeSettings,
  confirmInstall,
  installedRow,
  openPlugins,
  openPrompt,
  prompt,
  promptInstall,
  setBlockHighRisk,
  waitForCard,
} from "../../helpers/score-ui.ts";
import { completeWizard } from "../../helpers/voice-ui.ts";

const GROWTH = "Fixture growth";
const MEDIUM = "Fixture score-medium";
const DEVELOPER = "Fixture score-high";
const OFFICIAL = "Fixture Strip";

const confirmBoxes = () => prompt().$$(".plugin-prompt-confirm");
const tick = () => prompt().$(".plugin-prompt-confirm input[type='checkbox']");

describe("plugins: confirmation box and the high-risk safety setting (AC-9.34, AC-9.35)", () => {
  let devFolder = "";

  before(async function () {
    this.timeout(20 * 60_000);
    const fixtures = await pluginenv.startPluginFixtures({ official: {}, thirdParty: {} });
    if (fixtures.thirdPartyUrl === undefined) throw new Error("no third-party repository");
    devFolder = mkdtempSync(join(tmpdir(), "mumcord-score-dev-"));
    await runOk("python3", [
      "-I",
      "-m",
      "zipfile",
      "-e",
      pluginenv.stagedPackage("test-score-high-1.0.0.mcplugin"),
      devFolder,
    ]);
    await app.restart({ env: pluginenv.appEnv() });
    await completeWizard("Alice");
    await addRepository(fixtures.thirdPartyUrl);
  });

  after(async () => {
    await pluginenv.stopPluginFixtures();
    if (devFolder !== "") rmSync(devFolder, { recursive: true, force: true });
  });

  it("AC-9.35: with the setting on, a High plugin shows Install disabled with “Blocked by your safety setting”", async () => {
    await setBlockHighRisk(true);
    await openPlugins("Browse");
    await waitForCard(GROWTH);
    expect(await chipLevel(card(GROWTH))).toBe("blocked");
    expect(await chipText(card(GROWTH))).toBe("Blocked by your safety setting");
    await openPrompt(GROWTH);
    await expect(prompt().$(".plugin-prompt-score")).toHaveText(
      expect.stringContaining("Install is disabled because “Don't allow high-risk plugins” is on."),
    );
    // The button reads plain "Install" (not "Install anyway") and stays disabled, tick or not.
    await expect(promptInstall()).toHaveText("Install");
    await expect(promptInstall()).toBeDisabled();
    if (await tick().isExisting()) {
      await tick().click();
      await expect(promptInstall()).toBeDisabled();
    }
    await cancelPrompt();
  });

  it("AC-9.35: official plugins are unaffected by the setting", async () => {
    await waitForCard(OFFICIAL);
    expect(await chipLevel(card(OFFICIAL))).toBe("official");
    await openPrompt(OFFICIAL);
    await expect(promptInstall()).toBeEnabled();
    await expect(prompt()).not.toHaveText(expect.stringContaining("Blocked by your safety setting"));
    await cancelPrompt();
  });

  it("AC-9.35: a High developer plugin loads and runs with the setting on", async () => {
    const id = await app.invoke("plugin_dev_load", { folder: devFolder });
    expect(id).toBe("test-score-high");
    await openPlugins("Installed");
    const row = installedRow(DEVELOPER);
    await row.waitForDisplayed({ timeout: 30_000 });
    expect(await chipLevel(row)).toBe("high");
    expect(await chipText(row)).toBe("High risk · 83 Developer plugin");
    await expect(row.$("input[role='switch']")).toBeSelected();
  });

  it("AC-9.34: a High plugin can't be installed until the one box is ticked (repository warning merged)", async () => {
    await setBlockHighRisk(false);
    await openPlugins("Browse");
    await waitForCard(GROWTH);
    expect(await chipText(card(GROWTH))).toBe("High risk · 80");
    await openPrompt(GROWTH);
    // High risk, own code with websites and the first install from this repository: one box.
    expect(await confirmBoxes().length).toBe(1);
    expect(await prompt().$$(".plugin-prompt-confirm input[type='checkbox']").length).toBe(1);
    await expect(prompt().$(".plugin-prompt-confirm label")).toHaveText(
      `I understand the risk and I trust the author of ${GROWTH}`,
    );
    await expect(promptInstall()).toHaveText("Install anyway");
    await expect(promptInstall()).toBeDisabled();
    await tick().click();
    await expect(promptInstall()).toBeEnabled();
    await tick().click();
    await expect(promptInstall()).toBeDisabled();
    await confirmInstall(GROWTH);
  });

  it("FR-9.80: after the first install, a Medium plugin with websites asks for “I understand” only", async () => {
    await openPrompt(MEDIUM);
    expect(await confirmBoxes().length).toBe(1);
    await expect(prompt().$(".plugin-prompt-confirm")).toHaveText(
      expect.stringContaining("This plugin brings its own code and talks to websites."),
    );
    await expect(prompt().$(".plugin-prompt-confirm")).not.toHaveText(
      expect.stringContaining("is a repository you added"),
    );
    await expect(prompt().$(".plugin-prompt-confirm label")).toHaveText("I understand");
    await expect(promptInstall()).toHaveText("Install");
    await expect(promptInstall()).toBeDisabled();
    await cancelPrompt();
  });

  it("AC-9.35: an installed High plugin keeps running when the setting is turned on", async () => {
    await setBlockHighRisk(true);
    const row = installedRow(GROWTH);
    await row.waitForDisplayed({ timeout: 30_000 });
    await expect(row.$("input[role='switch']")).toBeSelected();
    await expect(row.$("input[role='switch']")).toBeEnabled();
    // Still enabled after a restart (the setting never switches plugins off).
    await closeSettings();
    await app.restart({ env: pluginenv.appEnv() });
    await openPlugins("Installed");
    await expect(installedRow(GROWTH).$("input[role='switch']")).toBeSelected();
    await expect($(".plugins-safety-note")).toBeDisplayed();
  });
});
