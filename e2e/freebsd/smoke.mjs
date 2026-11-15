#!/usr/bin/env node
// Native FreeBSD E2E: node e2e/freebsd/smoke.mjs [path/to/mumcord]
// Requires npm ci, Xvfb, WebKitWebDriver-4.1, dbus and mumble-server. No Docker or root.
// Uses a private display, session bus, loopback server and two disposable profiles. Never
// points at the desktop user's profile, keyring or server. Linux's E2E runner is unchanged.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import {
  mkdirSync,
  mkdtempSync,
  openSync,
  closeSync,
  rmSync,
  writeFileSync,
  readdirSync,
  readFileSync,
  copyFileSync,
  existsSync,
} from "node:fs";
import { createServer, createConnection } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import process from "node:process";
import { setTimeout as delay } from "node:timers/promises";
import { remote } from "webdriverio";

assert.equal(process.platform, "freebsd", "this runner is FreeBSD-only");
const binary = resolve(process.argv[2] ?? "target/release/mumcord");
const root = mkdtempSync(join(tmpdir(), "mumcord-freebsd-e2e-"));
const children = [];
const browsers = [];
const envs = [];
const artifacts = resolve("e2e/artifacts/freebsd");
mkdirSync(artifacts, { recursive: true });

function profile(name) {
  const home = join(root, name);
  const env = { ...process.env, HOME: home, TAURI_WEBVIEW_AUTOMATION: "true" };
  for (const [key, dir] of Object.entries({
    XDG_CONFIG_HOME: "config",
    XDG_DATA_HOME: "data",
    XDG_CACHE_HOME: "cache",
    XDG_RUNTIME_DIR: "runtime",
  })) {
    env[key] = join(home, dir);
    mkdirSync(env[key], { recursive: true, mode: 0o700 });
  }
  return env;
}

function start(command, args, env, name, extra = []) {
  const log = openSync(join(artifacts, `${name}.log`), "w", 0o600);
  try {
    const child = spawn(command, args, { env, stdio: ["ignore", log, log, ...extra] });
    children.push(child);
    child.on("error", (error) => console.error(`${name}: ${error.message}`));
    return child;
  } finally {
    closeSync(log);
  }
}

async function firstLine(stream) {
  const timeout = new AbortController();
  const deadline = delay(15_000, undefined, { signal: timeout.signal }).then(() => {
    stream.destroy();
    throw new Error("service did not report readiness within 15 seconds");
  });
  try {
    return await Promise.race([
      deadline,
      (async () => {
        let text = "";
        for await (const data of stream) {
          text += data.toString();
          if (text.includes("\n")) return text.split("\n")[0];
        }
        throw new Error("service ended before reporting readiness");
      })(),
    ]);
  } finally {
    timeout.abort();
  }
}

async function freePort() {
  const server = createServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const { port } = server.address();
  await new Promise((done) => server.close(done));
  return port;
}

async function ready(child, port) {
  for (let attempt = 0; attempt < 100; attempt++) {
    assert.equal(child.exitCode, null, `service on ${port} exited`);
    const connected = await new Promise((done) => {
      const socket = createConnection({ host: "127.0.0.1", port });
      socket.setTimeout(500);
      socket.once("connect", () => {
        socket.destroy();
        done(true);
      });
      socket.once("error", () => done(false));
      socket.once("timeout", () => {
        socket.destroy();
        done(false);
      });
    });
    if (connected) return;
    await delay(100);
  }
  throw new Error(`service on ${port} did not start`);
}

async function button(browser, text) {
  const element = browser.$(
    `//button[normalize-space(.)='${text}' or normalize-space(.)='${text} →']`,
  );
  await element.waitForClickable({ timeout: 30_000 });
  await element.click();
}

async function wizard(browser, name) {
  const primary = () => browser.$("form.wizard-step .wizard-footer button[type='submit']");
  await primary().waitForClickable({ timeout: 30_000 });
  await primary().click();
  const nickname = browser.$("input[autocomplete='nickname']");
  await nickname.waitForDisplayed({ timeout: 30_000 });
  await nickname.setValue(name);
  await primary().click();
  await browser
    .$("//p[contains(@class,'wizard-eyebrow')][contains(.,'Voice')]")
    .waitForExist({ timeout: 120_000 });
  await primary().click();
  await browser.$("label.preset-card[data-preset='everythingOn']").click();
  await button(browser, "Finish setup");
  await button(browser, "Start Mumcord");
  await browser.$("button[aria-label='Add a server']").waitForExist({ timeout: 30_000 });
  const offer = browser.$("[role='region'][aria-label='Aaron, the tips helper']");
  if (await offer.waitForDisplayed({ timeout: 3000 }).catch(() => false))
    await browser.keys("Escape");
}

async function connect(browser, port) {
  const address = browser.$("//label[.='Server address or mumble:// link']/following::input[1]");
  await address.waitForDisplayed({ timeout: 30_000 });
  await address.setValue(`127.0.0.1:${port}`);
  await button(browser, "Add and connect");
  await button(browser, "Trust and connect");
  await browser
    .$("//button[contains(@aria-label,'(connected)')]")
    .waitForExist({ timeout: 30_000 });
}

async function invoke(browser, command, args = {}) {
  const result = await browser.execute(
    async (command, args) => {
      try {
        return { value: await window.__TAURI_INTERNALS__.invoke(command, args) };
      } catch (error) {
        return { error };
      }
    },
    command,
    args,
  );
  assert.equal(result.error, undefined, `${command}: ${JSON.stringify(result.error)}`);
  return result.value;
}

try {
  const env = profile("services");
  const bus = start("dbus-daemon", ["--session", "--nofork", "--print-address=3"], env, "bus", [
    "pipe",
  ]);
  env.DBUS_SESSION_BUS_ADDRESS = await firstLine(bus.stdio[3]);
  const display = start(
    "Xvfb",
    ["-displayfd", "3", "-screen", "0", "1920x1080x24", "-nolisten", "tcp"],
    env,
    "display",
    ["pipe"],
  );
  env.DISPLAY = `:${await firstLine(display.stdio[3])}`;
  const port = await freePort();
  const ini = join(root, "server.ini");
  writeFileSync(
    ini,
    `host=127.0.0.1\nport=${port}\ndatabase=${root}/server.sqlite\nlogfile=\nregisterName=\nbonjour=false\nautobanAttempts=0\n`,
    { mode: 0o600 },
  );
  const server = start("mumble-server", ["-ini", ini, "-fg"], env, "server");
  await ready(server, port);

  for (const name of ["FreeBSD-A", "FreeBSD-B"]) {
    const clientEnv = {
      ...env,
      ...profile(name),
      DISPLAY: env.DISPLAY,
      DBUS_SESSION_BUS_ADDRESS: env.DBUS_SESSION_BUS_ADDRESS,
    };
    envs.push(clientEnv);
    const driverPort = await freePort();
    const driver = start(
      "WebKitWebDriver-4.1",
      [`--port=${driverPort}`, "--host=local"],
      clientEnv,
      name,
    );
    await ready(driver, driverPort);
    const browser = await remote({
      hostname: "127.0.0.1",
      port: driverPort,
      logLevel: "error",
      capabilities: {
        "wdio:enforceWebDriverClassic": true,
        browserName: "wry",
        "webkitgtk:browserOptions": { binary },
      },
    });
    browsers.push(browser);
    await browser.setWindowSize(1400, 900);
    await wizard(browser, name);
    await connect(browser, port);
    console.log(`PASS ${name}: fresh-profile wizard, certificate trust, local connection`);
  }

  for (const [sender, receiver, text] of [
    [browsers[0], browsers[1], "FreeBSD E2E A to B"],
    [browsers[1], browsers[0], "FreeBSD E2E B to A"],
  ]) {
    const composer = sender.$("textarea[aria-label^='Message']");
    await composer.waitForEnabled({ timeout: 15_000 });
    await composer.setValue(text);
    await sender.keys("Enter");
    await receiver.waitUntil(
      async () => (await receiver.$(".chat-list__items").getText()).includes(text),
      { timeout: 15_000 },
    );
  }
  console.log("PASS two-way chat between native clients");

  const [a, b] = browsers;
  for (const browser of browsers) {
    const settings = await invoke(browser, "settings_get");
    await invoke(browser, "settings_update", {
      settings: { ...settings, allowDirectConnections: "yes" },
    });
    const joinVoice = browser.$(".userpanel-readonly__join");
    if (await joinVoice.isExisting()) await joinVoice.click();
    await browser.waitUntil(async () => (await invoke(browser, "voice_server_get")) !== null, {
      timeout: 15_000,
    });
    await invoke(browser, "set_self_mute", { muted: true });
  }
  const serverId = await invoke(a, "voice_server_get");
  assert.equal(await invoke(a, "camera_unavailable", { serverId }), "unsupportedPlatform");
  assert.equal(await invoke(a, "screen_share_sound_support"), "platform");
  const update = await invoke(a, "update_check_now");
  assert.equal(update.install, "pkg");
  assert.equal(update.canInstall, false);
  assert.equal(update.lastCheckMs, null);
  console.log("PASS FreeBSD camera/sound limits and package-managed update status");
  await a.$("button.chat-apps__button").click();
  await a.keys("Home");
  await a.keys("Enter");
  const choose = a.$("//dialog//button[normalize-space()='Choose what to share']");
  await choose.waitForClickable({ timeout: 20_000 });
  assert.match(await a.$("dialog[open]").getText(), /whole X11 screen and carries no sound/);
  await choose.click();
  await a.$(".sharer-bar").waitForExist({ timeout: 30_000 });
  await b.$("//div[contains(@class,'activity-banner')]//button[normalize-space()='Watch']").click();
  let quality;
  await a.waitUntil(
    async () => {
      quality = await invoke(a, "screen_share_quality", { serverId });
      return (
        quality?.fps > 0 &&
        quality.viewers.some((viewer) => viewer.fps > 0 && viewer.path === "direct")
      );
    },
    {
      timeout: 60_000,
      timeoutMsg: "viewer never reported decoded frames over the direct connection",
    },
  );
  await delay(10_000);
  quality = await invoke(a, "screen_share_quality", { serverId });
  assert(quality.viewers.some((viewer) => viewer.fps > 0 && viewer.path === "direct"));
  const own = (await invoke(a, "screen_share_list", { serverId }))[0];
  assert.equal(own.kind, "screen");
  assert.equal(own.sound, false);
  console.log(
    `PASS whole-screen X11 capture and direct playback: ${quality.width}x${quality.height}, ${quality.fps} fps`,
  );
  await b.saveScreenshot(join(artifacts, "screen-viewer.png"));
  await invoke(a, "screen_share_set_paused", { serverId, paused: true });
  await b.waitUntil(async () => (await invoke(b, "screen_share_list", { serverId }))[0]?.paused, {
    timeout: 10_000,
  });
  await invoke(a, "screen_share_set_paused", { serverId, paused: false });
  await b.waitUntil(
    async () => (await invoke(b, "screen_share_list", { serverId }))[0]?.paused === false,
    {
      timeout: 10_000,
    },
  );
  await invoke(a, "screen_share_stop", { serverId });
  await b.waitUntil(async () => (await invoke(b, "screen_share_list", { serverId })).length === 0, {
    timeout: 10_000,
  });
  console.log("PASS share pause, resume and stop");
  await a.$("button[aria-label='Quick settings']").click();
  await a.$("//button[@role='menuitem'][contains(.,'All settings')]").click();
  await a.$("//button[@role='tab'][.//span[normalize-space(.)='About']]").click();
  await a.waitUntil(
    async () => (await a.$("body").getText()).includes("Not available on FreeBSD"),
    { timeout: 15_000 },
  );
  await a.saveScreenshot(join(artifacts, "about.png"));
  console.log("PASS FreeBSD About note and unsupported share sound");
  await a.keys("Escape");

  const second = start(binary, [`mumble://127.0.0.1:${port}/`], envs[0], "second-launch");
  const [status] = await Promise.race([
    once(second, "exit"),
    delay(5000).then(() => {
      throw new Error("second launch opened another instance");
    }),
  ]);
  assert.equal(status, 0);
  const logDir = join(envs[0].XDG_DATA_HOME, "app.mumcord.client", "logs");
  await a.waitUntil(
    () =>
      Promise.resolve(
        readdirSync(logDir)
          .filter((name) => name.endsWith(".log"))
          .some((name) =>
            readFileSync(join(logDir, name), "utf8").includes("mumble:// link received"),
          ),
      ),
    { timeout: 15_000 },
  );
  assert.equal((await invoke(a, "connections_list")).length, 1);
  console.log("PASS second launch hands off its link and exits without another connection");
  console.log("PASS FreeBSD native E2E");
} catch (error) {
  for (const [index, browser] of browsers.entries()) {
    await browser.saveScreenshot(join(artifacts, `failure-${index}.png`)).catch((error) => {
      console.error(`Could not save failure screenshot: ${error.message}`);
    });
  }
  throw error;
} finally {
  for (const browser of browsers.reverse()) {
    await browser.deleteSession().catch((error) => {
      console.error(`Could not close WebDriver session: ${error.message}`);
    });
  }
  for (const child of children.reverse()) {
    if (child.exitCode === null && child.signalCode === null && child.pid) {
      child.kill();
      await Promise.race([once(child, "exit"), delay(2000)]);
      if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
    }
  }
  // Only the app's redacted diagnostic logs leave the disposable profiles. Never copy the
  // databases, certificates, encrypted vault or its key into E2E artifacts.
  try {
    for (const [index, env] of envs.entries()) {
      const dir = join(env.XDG_DATA_HOME, "app.mumcord.client", "logs");
      for (const name of existsSync(dir) ? readdirSync(dir) : []) {
        if (name.endsWith(".log"))
          copyFileSync(join(dir, name), join(artifacts, `client-${index}-${name}`));
      }
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
