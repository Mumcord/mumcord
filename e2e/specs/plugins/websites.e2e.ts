// AC-9.7 … AC-9.10 (spec 09): plugin websites and `net.http`, one app against the third-party
// fixture repository, the local HTTPS fixture (declared.plugins.test / undeclared.plugins.test on
// 127.0.0.1:47443) and the local test server.
// - AC-9.9: every hard-stop fixture (test-stop-*) and the packaged test-dev-localhost are refused
//   in the install prompt with their reason, no score and no Install button.
// - AC-9.7: test-surface's `net.http` reaches its declared origin; the undeclared origin returns
//   `origin-not-allowed`; a redirect to the undeclared origin fails. With the public-address hook
//   off, the declared name resolved to 127.0.0.1 (MUMCORD_TEST_RESOLVE) fails with
//   `host-not-allowed` (the DNS rebinding case).
// - AC-9.8: the plugin's details then list its website and purpose, the approval date, the last
//   request and today's count with the blocked ones.
// - AC-9.10: test-dev-localhost loaded unpacked in Developer mode reaches http://localhost:47180.
//   The test loads the folder through the `plugin_dev_load` command, because the native folder
//   picker can't be driven in the container; picking a folder by hand stays a manual check.
import { createServer, type Server } from "node:http";
import { mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { app, pluginenv, testenv } from "../../helpers/index.ts";
import { runOk } from "../../helpers/run.ts";
import { fill } from "../../helpers/servers-ui.ts";
import { openSettingsGroup, turnOnDeveloperMode } from "../../helpers/settings-ui.ts";
import { addAndConnect, completeWizard, reconnect } from "../../helpers/voice-ui.ts";

const SURFACE = "Test Surface";
const DEV = { id: "test-dev-localhost", name: "Test Dev Localhost" };
/** The origin test-dev-localhost declares; its port is kept free by the HTTPS fixture. */
const DEV_PORT = 47180;
const DECLARED_HOST = "declared.plugins.test";

/** Hard-stop fixtures by name, with the reason the prompt gives (crates/plugins/src/origins.rs). */
const HARD_STOPS: [name: string, reason: string][] = [
  ["Fixture stop-http", "it wants to use a website that is not encrypted"],
  ["Fixture stop-ip", "it wants to use a website given as a number (IP address)"],
  ["Fixture stop-private", "it wants to use a website on your own computer or network"],
  ["Fixture stop-userinfo", "it wants to use a website address with a user name or password"],
  ["Fixture stop-wildcard", "it wants to use a whole group of websites (wildcard)"],
  // A localhost origin is only allowed for an unpacked developer plugin (AC-9.10).
  [DEV.name, "it wants to use a website that is not encrypted"],
];

const prompt = "//div[contains(concat(' ',@class,' '),' plugin-prompt ')]";
const pluginTab = (name: string) =>
  $(`//button[@role='tab'][contains(@class,'plugins-tab')][starts-with(normalize-space(.),'${name}')]`);
const settingsOverlay = () => $(".overlay__dialog");
const panel = () => $(".pstrip-panel");

async function closeSettings(): Promise<void> {
  await browser.keys(["Escape"]);
  await settingsOverlay().waitForDisplayed({ reverse: true, timeout: 10_000 });
}

/** Opens Browse and clicks Install on `name`; returns once the prompt shows. */
async function openPrompt(name: string): Promise<void> {
  await pluginTab("Browse").click();
  const button = $(`button[aria-label='Install ${name}']`);
  await button.waitForExist({ timeout: 30_000 });
  await button.scrollIntoView({ block: "center" });
  await button.waitForClickable({ timeout: 30_000 });
  await button.click();
  await $(prompt).waitForDisplayed({ timeout: 30_000 });
}

/** Opens a plugin's inline panel in the strip (no-op when it is open). */
async function openPanel(plugin: string): Promise<void> {
  const icon = $(`.pstrip-icons button[aria-label='${plugin}']`);
  await icon.waitForClickable({ timeout: 30_000 });
  for (let i = 0; i < 3; i++) {
    const open = (await icon.getAttribute("aria-pressed")) === "true";
    if (open && (await panel().isDisplayed())) return;
    await icon.click();
    await panel().waitForDisplayed({ timeout: 3_000 }).catch(() => undefined);
  }
  throw new Error(`the ${plugin} panel did not open`);
}

/** Clicks a button of the open plugin panel and waits for `line` in the panel. */
async function panelAction(label: string, line: string): Promise<void> {
  const button = panel().$(`button=${label}`);
  await button.scrollIntoView({ block: "center" });
  await button.click();
  await browser.waitUntil(async () => (await panel().getText()).includes(line), {
    timeout: 30_000,
    timeoutMsg: `"${line}" never showed in the panel: ${await panel().getText()}`,
  });
}

/** Answers every request on localhost:DEV_PORT with 200, on IPv4 and IPv6. */
function startDevServer(): Promise<Server> {
  const server = createServer((_request, response) => {
    response.writeHead(200, { "content-type": "text/plain" });
    response.end("ok localhost");
  });
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(DEV_PORT, () => {
      resolve(server);
    });
  });
}

/** Unpacks the built test-dev-localhost package into a fresh folder for Developer mode. */
async function unpackDevPlugin(): Promise<string> {
  const root = process.env.MUMCORD_E2E_PLUGIN_PACKAGES;
  if (root === undefined || root === "") {
    throw new Error("MUMCORD_E2E_PLUGIN_PACKAGES is not set (scripts/e2e-plugin-registry.sh)");
  }
  const packages = join(root, "packages", "repo");
  const file = readdirSync(packages).find((f) => f.startsWith(DEV.id) && f.endsWith(".mcplugin"));
  if (file === undefined) throw new Error(`no ${DEV.id} package in ${packages}`);
  const folder = mkdtempSync(join(tmpdir(), "mumcord-dev-plugin-"));
  await runOk("python3", ["-m", "zipfile", "-e", join(packages, file), folder]);
  return folder;
}

describe("plugins: websites and net.http (AC-9.7 … AC-9.10)", function () {
  this.bail(true);
  let fixtures: pluginenv.PluginFixtures;
  let devServer: Server | undefined;
  let devFolder: string | undefined;

  before(async function () {
    this.timeout(20 * 60_000);
    await testenv.up("v15");
    // test-surface 1.1.1 (one more website) is for the update gate (AC-9.36). Holding it back
    // makes the install pick 1.1.0 (FR-9.16), so AC-9.8 sees exactly the one declared website.
    fixtures = await pluginenv.startPluginFixtures({
      thirdParty: { scenarios: ["min-mumcord:test-surface@1.1.1=99.0"] },
      https: true,
    });
    await app.restart({ env: pluginenv.appEnv() });
    await completeWizard("Alice");
    // The Plugins strip lives in the channel panel, so the app is connected.
    await addAndConnect();
    // The fixture repository serves plain http on 127.0.0.1, which needs Developer mode; AC-9.10
    // needs it too.
    await turnOnDeveloperMode();
    await openSettingsGroup("Plugins");
    await pluginTab("Repositories").click();
    await $("button=Add repository").click();
    await fill("Repository address", fixtures.thirdPartyUrl ?? "");
    await $("button=Check repository").click();
    const confirm = $("button.settings-button--danger=Add repository");
    await confirm.waitForClickable({ timeout: 30_000 });
    await confirm.click();
    await confirm.waitForDisplayed({ reverse: true, timeout: 10_000 });
  });

  after(async () => {
    devServer?.close();
    if (devFolder !== undefined) rmSync(devFolder, { recursive: true, force: true });
    await pluginenv.stopPluginFixtures();
  });

  for (const [name, reason] of HARD_STOPS) {
    it(`AC-9.9: ${name} is refused with its reason and no score`, async () => {
      await openPrompt(name);
      const refused = $(`${prompt}//div[contains(@class,'plugin-prompt-refused')]`);
      await refused.waitForDisplayed({ timeout: 60_000 });
      await expect(refused).toHaveText(expect.stringContaining(`Cannot be installed: ${reason}.`));
      await expect($(`${prompt}//*[@role='meter']`)).not.toBeExisting();
      await expect($(`${prompt}//button[normalize-space(.)='Install']`)).not.toBeExisting();
      await $(`${prompt}//button[normalize-space(.)='Cancel']`).click();
      await $(prompt).waitForDisplayed({ reverse: true, timeout: 10_000 });
    });
  }

  it("AC-9.7: net.http reaches the declared origin and nothing else", async () => {
    await openPrompt(SURFACE);
    // Own code and a website make it high risk: one box to tick before Install.
    const box = $(`${prompt}//label[contains(@class,'plugin-prompt-check')]//input[@type='checkbox']`);
    await box.waitForClickable({ timeout: 60_000 });
    await box.click();
    const install = $(`${prompt}//button[normalize-space(.)='Install']`);
    await install.waitForClickable({ timeout: 10_000 });
    await install.click();
    await expect($(".plugin-toast")).toHaveText(expect.stringContaining(`${SURFACE} installed`), {
      wait: 60_000,
    });
    await closeSettings();

    await openPanel(SURFACE);
    await panelAction("HTTP declared", "http-declared: 200");
    await panelAction("HTTP undeclared", "http-undeclared: origin-not-allowed");
    // The declared host redirects to the undeclared one; redirects are never followed.
    await panelAction("HTTP redirect", "http-redirect: redirect");
  });

  it("AC-9.7: a declared name that resolves to a local address is blocked", async () => {
    // Without MUMCORD_TEST_PUBLIC_ADDR the fixture's 127.0.0.1 counts as local, as a rebinding
    // DNS answer would.
    const env = pluginenv.appEnv({ MUMCORD_TEST_RESOLVE: `${DECLARED_HOST}=127.0.0.1` });
    delete env.MUMCORD_TEST_PUBLIC_ADDR;
    await app.restart({ env });
    await reconnect();
    await openPanel(SURFACE);
    await panelAction("HTTP declared", "http-declared: host-not-allowed");
  });

  it("AC-9.8: the details show the website, approval date, last request and today's count", async () => {
    await openSettingsGroup("Plugins");
    await pluginTab("Installed").click();
    const row = $(
      `//ul[contains(@class,'plugins-installed-list')]/li[.//*[normalize-space(.)='${SURFACE}']]`,
    );
    await row.$("button=Permissions").click();
    const origins = row.$("ul.ps-origins");
    await origins.waitForDisplayed({ timeout: 30_000 });
    const sites = await origins.$$("li.ps-origin").map((li) => li.getText());
    expect(sites).toHaveLength(1);
    expect(sites[0]).toContain(fixtures.httpsOrigins?.declared ?? "");
    // The purpose is the plugin's own text; the manifest key must not leak through.
    expect(sites[0]).not.toContain("origins.declared");

    const today = new Intl.DateTimeFormat("en", { dateStyle: "medium" }).format(new Date());
    await expect(row.$(".pd-hint*=You approved them on")).toHaveText(
      expect.stringContaining(today),
    );
    const fact = (label: string) =>
      row.$(`//dl[contains(@class,'ps-facts')]/dt[normalize-space(.)='${label}']/following-sibling::dd[1]`);
    await expect(fact("Last request")).toHaveText(expect.stringContaining(DECLARED_HOST));
    // declared (ok), undeclared (blocked before sending), redirect (sent), resolved to a local
    // address (blocked).
    await expect(fact("Requests today")).toHaveText("4 (blocked: 2)");
    await closeSettings();
  });

  it("AC-9.10: an unpacked developer plugin reaches http://localhost in Developer mode", async () => {
    devServer = await startDevServer();
    devFolder = await unpackDevPlugin();
    const id = await browser.execute((folder: string) => {
      const w = window as unknown as {
        __TAURI_INTERNALS__: { invoke: (c: string, a: object) => Promise<unknown> };
      };
      return w.__TAURI_INTERNALS__.invoke("plugin_dev_load", { folder });
    }, devFolder);
    expect(id).toBe(DEV.id);
    await openSettingsGroup("Plugins");
    await pluginTab("Developer").click();
    await expect($(`.plugins-developer-list*=${DEV.name}`)).toBeDisplayed({ wait: 30_000 });
    await closeSettings();

    await openPanel(DEV.name);
    await panelAction("Fetch localhost", "Result: 200");
  });
});
