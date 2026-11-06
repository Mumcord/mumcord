// The `test-surface` fixture plugin (e2e/fixtures/plugins/test-surface) in a running app: one
// setup for the spec 09 web frame specs (third-party fixture repository plus the HTTPS fixture)
// and the controls of its panel. The panel's buttons are user gestures for the plugin, and the
// panel lists every probe result as `<place>/<probe>: <status>`, so a spec reads the results
// without entering a frame.
import { $, browser, expect } from "@wdio/globals";
import { addFirstServer, finishWizard } from "./chat-ui.ts";
import * as app from "./app.ts";
import * as pluginenv from "./pluginenv.ts";
import { openSettingsGroup } from "./settings-ui.ts";
import * as testenv from "./testenv.ts";

export const ID = "test-surface";
export const NAME = "Test Surface";

/**
 * Calls a backend command; a rejection comes back as its text instead of a thrown error (the
 * WebKit driver turns a rejected command into a WebDriver error even when the page catches it).
 */
export async function call(
  command: string,
  args: object = {},
): Promise<{ ok: unknown; error?: string }> {
  try {
    return { ok: await app.invoke(command, args) };
  } catch (e) {
    return { ok: null, error: e instanceof Error ? e.message : String(e) };
  }
}

export async function invokeOk(command: string, args: object = {}): Promise<unknown> {
  const result = await call(command, args);
  if (result.error !== undefined) throw new Error(`${command} failed: ${result.error}`);
  return result.ok;
}

async function status(): Promise<string | undefined> {
  const list = (await invokeOk("plugins_list")) as {
    plugins: { id: string; status: { kind: string } }[];
  };
  return list.plugins.find((p) => p.id === ID)?.status.kind;
}

/**
 * Starts the fixtures and an app connected to the v15 test server in Developer mode (the fixture
 * repository is plain http on localhost), adds the third-party fixture repository and installs
 * `test-surface` the way a confirmed install prompt does. `launch.audio` is the app's audio
 * backend (see `app.restart`). Call `pluginenv.stopPluginFixtures()`
 * in `after`.
 */
export async function setUp(
  launch: { env?: Record<string, string>; audio?: string } = {},
): Promise<pluginenv.PluginFixtures> {
  const fixtures = await pluginenv.startPluginFixtures({ thirdParty: {}, https: true });
  await testenv.up("v15");
  await app.restart({ env: pluginenv.appEnv(launch.env), audio: launch.audio });
  await finishWizard("Alice");
  await addFirstServer(testenv.ports.v15);
  const join = $(".userpanel-readonly__join");
  await join.waitForClickable({ timeout: 15_000 });
  await join.click();
  await openSettingsGroup("Advanced & Developer");
  const developer = $("//input[@role='switch'][@id=//label[.='Developer mode']/@for]");
  await developer.waitForExist({ timeout: 10_000 });
  await developer.click();
  await expect(developer).toBeSelected();
  await browser.keys("Escape");
  await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
  await browser.waitUntil(
    async () =>
      (await call("plugin_repo_add", { url: fixtures.thirdPartyUrl, expectedKey: null })).error ===
      undefined,
    { timeout: 10_000, timeoutMsg: "the fixture repository could not be added" },
  );
  await invokeOk("plugins_index", { language: "en" });
  await invokeOk("plugin_install_prepare", { id: ID });
  await invokeOk("plugin_install", {
    id: ID,
    version: null,
    optional: [],
    visibility: "nobody",
    confirmed: true,
  });
  await browser.waitUntil(async () => (await status()) === "running", {
    timeout: 30_000,
    timeoutMsg: `${ID} never started`,
  });
  return fixtures;
}

/** Opens the plugin's panel in the Plugins strip (no-op when it is open). */
export async function openPanel(): Promise<void> {
  const icon = $(`button.pstrip-icon[aria-label='${NAME}']`);
  await icon.waitForExist({ timeout: 20_000, timeoutMsg: `${NAME} has no strip icon` });
  // An open panel stays readable while a modal (the plugin's web dialog) covers the strip, so
  // only a closed panel needs the icon to be clickable.
  if ((await icon.getAttribute("aria-pressed")) === "true") return;
  await icon.waitForClickable({ timeout: 20_000, timeoutMsg: `${NAME}'s strip icon is covered` });
  await icon.click();
}

/** Clicks a button of the plugin's panel (a user gesture for the plugin). */
export async function panelButton(label: string): Promise<void> {
  await openPanel();
  const button = $(`//*[contains(@class,'pstrip-body')]//button[normalize-space()='${label}']`);
  await button.waitForClickable({ timeout: 10_000, timeoutMsg: `the panel has no "${label}"` });
  await button.click();
}

/**
 * The panel's text lines (results and log); empty while it is closed. Read from the DOM, not with
 * `getText`, which drops lines scrolled out of the panel's visible area.
 */
export async function panelLines(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll(".pstrip-body .pr-text"), (el) => el.textContent.trim()),
  );
}

/** Waits until the panel shows a line matching `pattern`; returns that line. */
export async function waitLine(pattern: RegExp, timeout = 15_000): Promise<string> {
  await openPanel();
  let lines: string[] = [];
  let line = "";
  await browser.waitUntil(
    async () => {
      lines = await panelLines();
      line = lines.find((l) => pattern.test(l)) ?? "";
      return line !== "";
    },
    {
      timeout,
      interval: 250,
      timeoutMsg: `the panel never showed ${String(pattern)}; it shows:\n${lines.join("\n")}`,
    },
  );
  return line;
}

/**
 * The probe results the pages reported for `place`, e.g. `{ ipc: "blocked" }`, once none of the
 * `probes` is missing or still `running`.
 */
export async function waitResults(
  place: string,
  probes: readonly string[],
  timeout = 180_000,
): Promise<Record<string, string>> {
  await openPanel();
  let results: Record<string, string> = {};
  let lines: string[] = [];
  const done = async () => {
    results = {};
    lines = await panelLines();
    for (const line of lines) {
      const match = /^([a-z-]+)\/([a-z-]+): ([a-z-]+)$/.exec(line.trim());
      if (match?.[1] === place) results[match[2]] = match[3];
    }
    return probes.every((p) => p in results && results[p] !== "running");
  };
  try {
    await browser.waitUntil(done, { timeout, interval: 500 });
  } catch {
    // Built here, not as `timeoutMsg`, which wdio reads before the first try.
    throw new Error(
      `${place} did not report every probe: ${JSON.stringify(results)}; the panel shows:\n${lines.join("\n")}`,
    );
  }
  return results;
}
