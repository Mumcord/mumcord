// Plugin API versions (spec 09, FR-9.13 … FR-9.19; issue #396): AC-9.26, AC-9.27, AC-9.28 and
// AC-9.30 against the fixture registry (official + third-party, each with `v1/` and `v1.1/`).
//
// - `test-versions` has 1.0.0 (API 1.0), 2.0.0 (API 1.1) and 3.0.0 (API 1.1, the index sets
//   `min_mumcord` to the next minor release): this build installs 2.0.0, the v1 index lists only
//   1.0.0, and 3.0.0 is held back as "Update available after updating Mumcord" with automatic updates on.
// - "Fixture Highlight" needs the next minor release on every version: "Needs Mumcord … or newer".
// - `test-api12` declares `api: "^1.2"` in its package; the index claims 1.1 so Browse offers it
//   and only the client's check of the downloaded package can refuse it (FR-9.17).
// - The third-party repository is added by its `v1/index.json`: without a `v1.1` sibling only
//   the API 1.0 plugin (Fixture score-low) shows, with it the API 1.1 one (Test Stage) does too.
//
// Manual (not on the Linux CI runner): a 0.6.x release build pointed at the fixture registry
// installs test-versions 1.0.0 and never sees 2.0.0, and refuses a local 1.1 package (AC-9.26,
// #396 AC 1 and 3). There is no 1.0-only client here, so the refusal is automated with an API
// this build does not know (^1.2).
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { $, $$, browser, expect } from "@wdio/globals";
import { app, pluginenv } from "../../helpers/index.ts";
import { fill } from "../../helpers/servers-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { completeWizard } from "../../helpers/voice-ui.ts";

const VERSIONS = "Fixture versions";
const HIGHLIGHT = "Fixture Highlight";
const API12 = "Fixture API 1.2";
const REPO_NAME = "E2E third-party";
const STAGE = "Test Stage";
const SCORE_LOW = "Fixture score-low";

const pluginTab = (name: string) =>
  $(`//button[@role='tab'][contains(@class,'plugins-tab')][starts-with(normalize-space(.),'${name}')]`);
const card = (name: string) =>
  $(`//li[@class='plugin-card'][.//*[contains(@class,'plugin-card-name')][normalize-space(.)='${name}']]`);
const prompt = () => $("//div[contains(concat(' ',@class,' '),' plugin-prompt ')]");
const pluginsDir = () => join(app.profile(), "data", "app.mumcord.client", "plugins");

interface IndexVersion {
  version: string;
  api: string | number;
  min_mumcord?: string;
}
interface IndexFile {
  plugins: { id: string; versions: IndexVersion[] }[];
}

async function index(url: string): Promise<IndexFile> {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${String(response.status)}`);
  return (await response.json()) as IndexFile;
}

function versionsOf(file: IndexFile, id: string): IndexVersion[] {
  const plugin = file.plugins.find((p) => p.id === id);
  if (plugin === undefined) throw new Error(`${id} is not in the index`);
  return plugin.versions;
}

async function openPlugins(tab: string): Promise<void> {
  await openSettingsGroup("Plugins");
  await pluginTab(tab).click();
}

async function browseNames(): Promise<string[]> {
  return $$("ul.plugin-browse-list > li.plugin-card .plugin-card-name").map((el) => el.getText());
}

/** Adds a repository; the Plugins settings are open. */
async function addRepository(url: string): Promise<void> {
  await pluginTab("Repositories").click();
  await $("button=Add repository").click();
  await fill("Repository address", url);
  await $("button=Check repository").click();
  const confirm = $("button.settings-button--danger=Add repository");
  await confirm.waitForClickable({ timeout: 30_000 });
  await confirm.click();
  await confirm.waitForDisplayed({ reverse: true, timeout: 10_000 });
}

describe("plugins: API versions and Needs Mumcord (AC-9.26 … AC-9.28, AC-9.30)", () => {
  let fixtures: pluginenv.PluginFixtures;
  let nextRelease: string;

  before(async function () {
    this.timeout(20 * 60_000);
    // Stay ahead of the running binary, not its display version or a hard-coded old release.
    const info = await app.invoke<{ packageVersion: string }>("app_info");
    const version = /^(\d+)\.(\d+)\./.exec(info.packageVersion);
    if (!version) throw new Error(`invalid Mumcord package version: ${info.packageVersion}`);
    nextRelease = `${version[1]}.${String(Number(version[2]) + 1)}`;
    fixtures = await pluginenv.startPluginFixtures({
      official: {
        scenarios: [
          `min-mumcord:test-versions@3.0.0=${nextRelease}`,
          `min-mumcord:echolings=${nextRelease}`,
          "index-api:test-api12=1.1",
        ],
      },
      thirdParty: { scenarios: ["no-v1.1"] },
    });
    await app.restart({ env: pluginenv.appEnv() });
    await completeWizard("Alice");
    // The fixture registry serves plain http on 127.0.0.1, which the client only accepts in
    // Developer mode (FR-6.74).
    await openSettingsGroup("Advanced & Developer");
    const developerMode = $("//input[@role='switch'][@id=//label[.='Developer mode']/@for]");
    await developerMode.waitForExist({ timeout: 10_000 });
    await developerMode.click();
    await expect(developerMode).toBeSelected();
  });

  after(async () => {
    await pluginenv.stopPluginFixtures();
  });

  it("AC-9.26: the v1 index lists only the API 1.0 version, v1.1 every version", async () => {
    const officialUrl = fixtures.officialUrl ?? "";
    const v1 = versionsOf(await index(`${officialUrl}index.json`), "test-versions");
    expect(v1.map((v) => v.version)).toEqual(["1.0.0"]);
    const v11 = versionsOf(
      await index(officialUrl.replace(/v1\/$/, "v1.1/index.json")),
      "test-versions",
    );
    expect(v11.map((v) => [v.version, v.api])).toEqual(
      expect.arrayContaining([
        ["1.0.0", "1.0"],
        ["2.0.0", "1.1"],
        ["3.0.0", "1.1"],
      ]),
    );
    expect(v11.find((v) => v.version === "3.0.0")?.min_mumcord).toBe(nextRelease);
  });

  it("AC-9.27: a plugin without a version for this Mumcord shows Needs Mumcord, Install disabled", async () => {
    await openPlugins("Browse");
    await card(HIGHLIGHT).waitForDisplayed({ timeout: 30_000 });
    await expect(card(HIGHLIGHT).$(".plugin-card-needs")).toHaveText(
      expect.stringContaining(`Needs Mumcord ${nextRelease}.0 or newer`),
    );
    await expect(card(HIGHLIGHT).$("button.plugin-card-needs-link")).toHaveText("Update Mumcord");
    await expect(card(HIGHLIGHT).$(`button[aria-label='Install ${HIGHLIGHT}']`)).toBeDisabled();
    // A plugin with a version that fits has no such note.
    await expect(card(VERSIONS).$(".plugin-card-needs")).not.toBeExisting();
  });

  it("AC-9.26: installs the newest version this build can run (2.0.0, API 1.1)", async () => {
    const install = card(VERSIONS).$(`button[aria-label='Install ${VERSIONS}']`);
    await install.waitForClickable({ timeout: 30_000 });
    await install.click();
    const confirm = prompt().$("button=Install");
    await confirm.waitForClickable({ timeout: 60_000 });
    await confirm.click();
    await expect($(".plugin-toast")).toHaveText(expect.stringContaining(`${VERSIONS} installed`), {
      wait: 60_000,
    });
    await pluginTab("Installed").click();
    const row = $(`//li[.//*[normalize-space(.)='${VERSIONS}']]//span[@class='plugins-installed-version']`);
    await expect(row).toHaveText(expect.stringContaining("2.0.0"), { wait: 10_000 });
    expect(existsSync(join(pluginsDir(), "test-versions"))).toBe(true);
  });

  it("AC-9.28: an update that needs a newer Mumcord is held, even with automatic updates", async () => {
    await pluginTab("Updates").click();
    await $("summary=Update settings").click();
    await $("//label[normalize-space(.)='Install automatically']").click();
    await expect($("//label[normalize-space(.)='Install automatically']/input[@type='radio']")).toBeSelected();
    await $("button=Check now").click();

    const update = $(`//li[@class='plugin-update'][.//span[@class='plugin-update__name'][.='${VERSIONS}']]`);
    await update.waitForDisplayed({ timeout: 60_000 });
    await expect(update.$(".plugin-update__meta")).toHaveText(expect.stringMatching(/2\.0\.0\s*→\s*3\.0\.0/));
    await expect(update.$(".plugin-update__hold")).toHaveText("Update available after updating Mumcord");
    await expect(update.$(".plugin-update__actions")).not.toBeExisting();

    // A second check still does not move it: the plugin stays on 2.0.0.
    await $("button=Check now").click();
    await browser.pause(3_000);
    await pluginTab("Installed").click();
    await expect(
      $(`//li[.//*[normalize-space(.)='${VERSIONS}']]//span[@class='plugins-installed-version']`),
    ).toHaveText(expect.stringContaining("2.0.0"));
  });

  it("AC-9.27: a package for a newer API is refused at install and nothing is written", async () => {
    await pluginTab("Browse").click();
    const install = card(API12).$(`button[aria-label='Install ${API12}']`);
    await install.waitForClickable({ timeout: 30_000 });
    await install.click();
    await expect(prompt().$("[role='alert']")).toHaveText(
      expect.stringContaining("Needs a newer Mumcord"),
      { wait: 60_000 },
    );
    await expect(prompt().$("button=Install")).not.toBeExisting();
    await browser.keys(["Escape"]);
    expect(readdirSync(pluginsDir()).filter((name) => name.includes("test-api12"))).toEqual([]);
  });

  it("AC-9.30: a third-party v1 address uses its v1.1 sibling, else the added file", async () => {
    // Without the sibling (HTTP 404) the client uses the added v1 index: API 1.0 plugins only.
    // Still in the Plugins settings (the sidebar entry now carries the Updates count).
    await addRepository(fixtures.thirdPartyUrl ?? "");
    await pluginTab("Browse").click();
    await card(SCORE_LOW).waitForDisplayed({ timeout: 30_000 });
    expect(await browseNames()).not.toContain(STAGE);

    // Remove it, publish the sibling and add the same address again: the 1.1 plugins show.
    await pluginTab("Repositories").click();
    await $(`button[aria-label='Remove ${REPO_NAME}']`).click();
    const confirmRemove = $("//div[contains(@class,'settings-dialog')]//button[normalize-space(.)='Remove']");
    await confirmRemove.waitForClickable({ timeout: 10_000 });
    await confirmRemove.click();
    await confirmRemove.waitForDisplayed({ reverse: true, timeout: 10_000 });
    await $("p=No repositories added yet.").waitForDisplayed({ timeout: 10_000 });
    await pluginenv.setScenarios("third-party", []);
    await addRepository(fixtures.thirdPartyUrl ?? "");
    await pluginTab("Browse").click();
    await card(STAGE).waitForDisplayed({ timeout: 30_000 });
    await expect(card(SCORE_LOW)).toBeDisplayed();
  });
});
