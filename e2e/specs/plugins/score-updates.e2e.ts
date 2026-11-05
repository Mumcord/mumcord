// AC-9.36 (spec 09 §4.12, FR-9.82, FR-9.83): automatic updates hold back an update whose risk
// goes up or that adds websites, and install a same-permission update on their own.
//
// The repositories start without the newer versions; the spec publishes them after installing:
// - test-growth 1.0.0 (Medium, 45) → 1.1.0 adds ui.webview and peers.relay (High, 80: 57 points
//   × 1.15 plus 14 for the two recently added permissions);
// - test-surface 1.1.0 → 1.1.1 adds the website https://update.plugins.test:47443;
// - test-versions 1.0.0 → 2.0.0 (official, same permissions) installs automatically.
// Update mode is the default, "Install automatically".
import { copyFileSync, mkdirSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { app, pluginenv } from "../../helpers/index.ts";
import {
  addRepository,
  chipText,
  confirmInstall,
  installedRow,
  openPlugins,
  openPrompt,
  waitForCard,
} from "../../helpers/score-ui.ts";
import { completeWizard } from "../../helpers/voice-ui.ts";

const GROWTH = "Fixture growth";
const SURFACE = "Test Surface";
const VERSIONS = "Fixture versions";

/** Newer versions a repository must not serve until the spec publishes them. */
const HELD_BACK = ["test-growth-1.1.0.mcplugin", "test-surface-1.1.1.mcplugin"];

/** A copy of the built packages of `folder` ("repo" or "official") keeping only `keep`. */
function packagesWithout(folder: string, keep: (file: string) => boolean): string {
  const root = process.env.MUMCORD_E2E_PLUGIN_PACKAGES;
  if (root === undefined || root === "") throw new Error("MUMCORD_E2E_PLUGIN_PACKAGES is not set");
  const source = join(root, "packages", folder);
  const target = mkdtempSync(join(tmpdir(), `mumcord-score-${folder}-`));
  mkdirSync(target, { recursive: true });
  for (const file of readdirSync(source).filter(keep)) {
    copyFileSync(join(source, file), join(target, file));
  }
  return target;
}

const updateRow = (name: string) =>
  $(`//li[contains(concat(' ',@class,' '),' plugin-update ')][.//span[contains(@class,'plugin-update__name')][normalize-space(.)='${name}']]`);
const reviewDialog = () => $("//*[contains(concat(' ',@class,' '),' plugin-review ')]");

async function checkNow(): Promise<void> {
  await openPlugins("Updates");
  const button = $("button=Check now");
  await button.waitForClickable({ timeout: 30_000 });
  await button.click();
  await expect($("p.plugin-updates__checked")).toHaveText(expect.stringContaining("Last checked"), {
    wait: 60_000,
  });
}

async function installedVersion(name: string): Promise<string> {
  const row = installedRow(name);
  await row.waitForDisplayed({ timeout: 30_000 });
  return (await row.$(".plugins-installed-version").getText()).split(" · ")[0] ?? "";
}

describe("plugins: update holds when the score goes up or websites are added (AC-9.36)", () => {
  const folders: string[] = [];

  before(async function () {
    this.timeout(20 * 60_000);
    const repo = packagesWithout("repo", (f) => f.endsWith(".mcplugin") && !HELD_BACK.includes(f));
    const official = packagesWithout("official", (f) => f === "test-versions-1.0.0.mcplugin");
    folders.push(repo, official);
    const fixtures = await pluginenv.startPluginFixtures({
      official: { packages: official },
      thirdParty: { packages: repo },
    });
    if (fixtures.thirdPartyUrl === undefined) throw new Error("no third-party repository");
    await app.restart({ env: pluginenv.appEnv() });
    await completeWizard("Alice");
    await addRepository(fixtures.thirdPartyUrl);
    await openPlugins("Browse");
    for (const name of [GROWTH, SURFACE, VERSIONS]) {
      await waitForCard(name);
      await openPrompt(name);
      await confirmInstall(name);
    }
    await openPlugins("Installed");
    expect(await installedVersion(GROWTH)).toBe("1.0.0");
    expect(await installedVersion(SURFACE)).toBe("1.1.0");
    expect(await installedVersion(VERSIONS)).toBe("1.0.0");

    await pluginenv.publish(pluginenv.stagedPackage("test-growth-1.1.0.mcplugin"), "third-party");
    await pluginenv.publish(pluginenv.stagedPackage("test-surface-1.1.1.mcplugin"), "third-party");
    await pluginenv.publish(pluginenv.stagedPackage("test-versions-2.0.0.mcplugin"), "official");
    await checkNow();
  });

  after(async () => {
    await pluginenv.stopPluginFixtures();
    for (const folder of folders) rmSync(folder, { recursive: true, force: true });
  });

  it("AC-9.36: a same-permission update installs automatically", async () => {
    await openPlugins("Installed");
    await browser.waitUntil(async () => (await installedVersion(VERSIONS)) === "2.0.0", {
      timeout: 60_000,
      timeoutMsg: "test-versions 2.0.0 was not installed automatically",
    });
  });

  it("AC-9.36: test-growth 1.1.0 (Medium → High) waits in Updates with before and after", async () => {
    await openPlugins("Updates");
    const row = updateRow(GROWTH);
    await row.waitForDisplayed({ timeout: 30_000 });
    await expect(row.$(".plugin-update__hold")).toHaveText(
      "Not installed automatically: the risk went up from Medium to High. Review it first.",
    );
    const score = row.$(".plugin-update__score");
    const chips = await score.$$(".risk-chip").map((chip) => chip.getAttribute("data-level"));
    expect(chips).toEqual(["medium", "high"]);
    await expect(score).toHaveText(expect.stringContaining("45"));
    await expect(score).toHaveText(expect.stringContaining("80"));
    await openPlugins("Installed");
    expect(await installedVersion(GROWTH)).toBe("1.0.0");
  });

  it("AC-9.36: test-surface 1.1.1 (new website) waits with the red website warning marking it “New”", async () => {
    await openPlugins("Updates");
    const row = updateRow(SURFACE);
    await row.waitForDisplayed({ timeout: 30_000 });
    await expect(row.$(".plugin-update__hold")).toHaveText(
      `This update wants to talk to 1 new website. ${SURFACE} stays on 1.1.0 until you decide.`,
    );
    await row.$("button*=Review and update").click();
    const dialog = reviewDialog();
    await dialog.waitForDisplayed({ timeout: 10_000 });
    await expect(dialog.$(".plugin-review__alert")).toHaveText(
      expect.stringContaining("This update wants to talk to 1 new website."),
    );
    const origins = await dialog.$$("ul.plugin-prompt-origins > li").map(async (li) => ({
      text: await li.getText(),
      isNew: await li.$(".plugin-prompt-origin__new").isExisting(),
    }));
    expect(origins.find((o) => o.text.includes("https://update.plugins.test:47443"))?.isNew).toBe(true);
    expect(origins.find((o) => o.text.includes("https://declared.plugins.test:47443"))?.isNew).toBe(
      false,
    );
    await expect(dialog.$(".perm-icon.perm-risk-red")).toBeExisting();
  });

  it("AC-9.36: Skip this version keeps the old version working", async () => {
    await reviewDialog().$("button=Skip this version").click();
    await reviewDialog().waitForDisplayed({ reverse: true, timeout: 10_000 });
    await expect(updateRow(SURFACE)).not.toBeExisting();
    await openPlugins("Installed");
    expect(await installedVersion(SURFACE)).toBe("1.1.0");
    await expect(installedRow(SURFACE).$("input[role='switch']")).toBeSelected();
  });

  it("AC-9.36: the tick plus Update anyway installs test-growth 1.1.0", async () => {
    await openPlugins("Updates");
    await updateRow(GROWTH).$("button*=Review and update").click();
    const dialog = reviewDialog();
    await dialog.waitForDisplayed({ timeout: 10_000 });
    await expect(dialog.$(".plugin-review__high")).toHaveText(
      expect.stringContaining("This update would make the plugin high risk"),
    );
    const update = dialog.$("button=Update anyway");
    await expect(update).toBeDisabled();
    await dialog.$(".plugin-review__tick input[type='checkbox']").click();
    await expect(update).toBeEnabled();
    await update.click();
    await dialog.waitForDisplayed({ reverse: true, timeout: 60_000 });
    await openPlugins("Installed");
    await browser.waitUntil(async () => (await installedVersion(GROWTH)) === "1.1.0", {
      timeout: 60_000,
      timeoutMsg: "test-growth 1.1.0 was not installed after Update anyway",
    });
    expect(await chipText(installedRow(GROWTH))).toBe("High risk · 80");
  });
});
