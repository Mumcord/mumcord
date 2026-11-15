// Spec 09 (plugin platform 1.1), plugin data through the relay and its Bovnar documents (#400),
// with two Mumcord users in the channel of the mumpeer test environment (helpers/relayenv.ts):
//
//   AC-9.18  profile `plugin-data`: A and B exchange 64 KiB `test-relay` messages 1:1 and to the
//            channel; the stage chip says "Relay · end-to-end encrypted"; /v1/status and the
//            relay's log show no plugin id and no payload; B leaving the channel stops getting
//            A's channel messages, and B back in the channel gets them again.
//   AC-9.41  the messages are Bovnar documents built and read by the SDK helpers: B's plugin
//            decodes every one with the values A sent ("intact", checked by its checksum), with
//            the relay and, without it, through the Mumble fallback.
//   AC-9.42  malformed bytes reach B's plugin unchanged and its SDK drops them with the reason
//            and size in the plugin log (no content); the valid message sent right after arrives;
//            an oversized document is refused by A's SDK and oversized raw bytes by A's host, so
//            nothing leaves A's machine.
//   AC-9.20  turning off the plugin's `peers.relay`, or the global switch, moves its traffic off
//            the relay at once (status `switched-off`, 64 KiB returns `relay-unavailable`, the
//            relay's sessions end); turning it on again brings the relay back.
//   AC-9.21  the settings card lists the server's relay as allowing plugin data and the data
//            Test Relay used this session.
//   AC-9.19  profiles `no-plugin-data` and `old` (the published mumpeer 0.1.0 with mumbot 0.4.1,
//            MUMCORD_E2E_OLD_MUMPEER_IMAGE): small messages arrive through Mumble plugin data,
//            64 KiB ones return `relay-unavailable`, the stage says "Mumble connection · slower"
//            and the settings card "relay doesn't allow plugin data".
//
// The relay bot hands out relay tokens to registered users only, so both users register.
// Client A = Alice (sender), B = Bob (receiver); test-relay comes from the third-party fixture
// repository (scripts/e2e-plugin-registry.sh, MUMCORD_E2E_PLUGIN_PACKAGES).
//
// Not automated here:
//   - AC-9.41's packet inspector view of the Bovnar bytes (manual check in the debug inspector),
//     and the typed document with units and a nested list, which needs the `test-bovnar`
//     fixture (spec 09 §10.2); test-relay's document carries a number and two strings.
//   - AC-9.42's wrong-typed and too deeply nested documents (`test-bovnar` error mode); the SDK's
//     unit tests cover them (crates/plugin-sdk/src/bovnar/tests.rs).
//   - The rekey itself when B leaves: the spec checks its effect (B gets nothing while away); the
//     key rotation is covered by the engine's plugin_relay tests.
import { readdirSync, readFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { app, apps, pluginenv, relayenv } from "../../helpers/index.ts";
import { waitForAbsent } from "../../helpers/dom.ts";
import { registerOnServer } from "../../helpers/files-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { run } from "../../helpers/run.ts";
import {
  addAndConnect,
  completeWizard,
  personRow,
  showAllPeople,
  SERVER,
} from "../../helpers/voice-ui.ts";

const ID = "test-relay";
const PLUGIN = "Test Relay";
const BIG = 64 * 1024;
const SMALL = 900;
const RELAY_CHIP = "Relay · end-to-end encrypted";
const MUMBLE_CHIP = "Mumble connection · slower";
/** A run of the test message's body (`a`…`z` repeated); it must never appear in any log. */
const BODY = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";

const oldImage = process.env.MUMCORD_E2E_OLD_MUMPEER_IMAGE;
const oldBotImage = process.env.MUMCORD_E2E_OLD_MUMBOT_IMAGE;

/**
 * Calls a backend command; a rejection comes back as its text instead of a thrown error (the
 * WebKit driver turns a rejected command into a WebDriver error even when the page catches it).
 */
async function call(command: string, args: object = {}): Promise<{ ok: unknown; error?: string }> {
  try {
    return { ok: await app.invoke(command, args) };
  } catch (e) {
    return { ok: null, error: e instanceof Error ? e.message : String(e) };
  }
}

async function invokeOk<T>(command: string, args: object = {}): Promise<T> {
  const result = await call(command, args);
  if (result.error !== undefined) throw new Error(`${command} failed: ${result.error}`);
  return result.ok as T;
}

/** Accessible name of the localhost server icon, including status and badges. */
async function serverLabel(): Promise<string> {
  return browser.execute((server: string) => {
    const icon = Array.from(document.querySelectorAll(".server-bar__server")).find((element) =>
      (element.getAttribute("aria-label") ?? "").startsWith(server),
    );
    return icon?.getAttribute("aria-label") ?? "";
  }, SERVER);
}

/**
 * Waits until the server icon says it is connected. The name also carries badges ("voice
 * server", unread), so an exact "localhost (connected)" never matches once voice is up. A
 * recreate can end the automatic reconnect (the server resets logins until it is up); the icon
 * is then clicked once, which connects again. A new certificate is trusted: this is the local
 * test server.
 */
async function waitServerUp(timeout: number): Promise<void> {
  let label = "";
  let nudged = false;
  await browser
    .waitUntil(
      async () => {
        const trustChanged = $("button=Trust the new certificate");
        const trustNew = $("button=Trust and connect");
        if ((await trustChanged.isExisting()) && (await trustChanged.isDisplayed())) {
          await trustChanged.click();
          return false;
        }
        if ((await trustNew.isExisting()) && (await trustNew.isDisplayed())) {
          await trustNew.click();
          return false;
        }
        label = await serverLabel();
        if (label.startsWith(`${SERVER} (connected)`)) return true;
        const gaveUp =
          label.startsWith(`${SERVER} (connection lost)`) ||
          label.startsWith(`${SERVER} (connection failed)`);
        if (!nudged && gaveUp) {
          nudged = true;
          await browser.execute((server: string) => {
            const icon = Array.from(document.querySelectorAll(".server-bar__server")).find(
              (element) => (element.getAttribute("aria-label") ?? "").startsWith(server),
            );
            if (icon instanceof HTMLElement) icon.click();
          }, SERVER);
        }
        return false;
      },
      { timeout, interval: 500 },
    )
    .catch(() => {
      throw new Error(`the server did not show connected (last label: "${label}")`);
    });
  await $("[role='tree']").waitForExist({ timeout: 15_000 });
  await showAllPeople();
}

/** Joins the shown server's channel when its read-only notice offers it. */
async function joinVoice(timeout = 30_000): Promise<void> {
  // Plugins only see the voice server. A reconnect that kept voice is already it; one that came
  // back read-only shows "Join voice" once the connection has synced.
  await waitServerUp(timeout);
  const join = $(".userpanel-readonly__join");
  const voice = async () => (await invokeOk<number | null>("voice_server_get")) !== null;
  await browser.waitUntil(async () => (await voice()) || (await join.isDisplayed()), {
    timeout,
    timeoutMsg: "the server is neither the voice server nor offers Join voice",
  });
  if (!(await voice())) {
    await join.click();
    await browser.waitUntil(voice, { timeout, timeoutMsg: "Join voice did not take voice" });
  }
}

/** Settings → Privacy & Connections: "Relay only", so plugin data has no direct path to take. */
async function relayOnly(): Promise<void> {
  await openSettingsGroup("Privacy & Connections");
  const radio = $("fieldset.p2p-modes input[type='radio'][value='relayOnly']");
  await radio.waitForExist();
  await radio.parentElement().click();
  await browser.waitUntil(async () => radio.isSelected(), { timeoutMsg: "relayOnly was not selected" });
  await browser.keys("Escape");
  await $("fieldset.p2p-modes").waitForExist({ reverse: true });
}

/**
 * Wizard, the relay environment's server, voice, registration, Relay only, Developer mode (the
 * fixture repository is plain http on localhost, which only Developer mode accepts), the
 * third-party repository and test-relay with `peers.relay` granted.
 */
async function setUp(name: string, port: number, repoUrl: string): Promise<void> {
  await completeWizard(name);
  await addAndConnect(`localhost:${String(port)}`);
  await joinVoice();
  await registerOnServer();
  await relayOnly();
  await openSettingsGroup("Advanced & Developer");
  const developer = $("//input[@role='switch'][@id=//label[.='Developer mode']/@for]");
  await developer.waitForExist({ timeout: 10_000 });
  await developer.click();
  await expect(developer).toBeSelected();
  await browser.keys("Escape");
  await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
  await browser.waitUntil(
    async () => (await call("plugin_repo_add", { url: repoUrl, expectedKey: null })).error === undefined,
    { timeout: 10_000, timeoutMsg: "the fixture repository could not be added" },
  );
  await invokeOk("plugins_index", { language: "en" });
  await invokeOk("plugin_install_prepare", { id: ID });
  await invokeOk("plugin_install", {
    id: ID,
    version: null,
    optional: ["peers.relay"],
    visibility: "everyone",
    confirmed: true,
  });
  await browser.waitUntil(
    async () => {
      const list = await invokeOk<{ plugins: { id: string; status: { kind: string } }[] }>(
        "plugins_list",
      );
      return list.plugins.find((p) => p.id === ID)?.status.kind === "running";
    },
    { timeout: 30_000, interval: 250, timeoutMsg: `${ID} never started` },
  );
}

/** Opens the plugin's panel in the strip (no-op when it is open). */
async function openPanel(): Promise<void> {
  const icon = $(`button.pstrip-icon[aria-label='${PLUGIN}']`);
  await icon.waitForExist({ timeout: 20_000, timeoutMsg: `${PLUGIN} has no strip icon` });
  if ((await icon.getAttribute("aria-pressed")) !== "true") {
    await icon.waitForClickable({ timeout: 10_000 });
    await icon.click();
  }
}

/**
 * The text rows of the open panel (status lines and log). Read from the DOM, because the log sits
 * at the bottom of the panel's scroll area and WebDriver's visible text leaves out what is
 * scrolled away.
 */
async function panelText(): Promise<string> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll(".pstrip-body .pr-text"))
      .map((line) => line.textContent.trim())
      .join("\n"),
  );
}

/** Waits until the panel shows a line containing `text` (or matching it). */
async function waitPanel(text: string | RegExp, timeout = 30_000): Promise<void> {
  await openPanel();
  let seen = "";
  await browser
    .waitUntil(
      async () => {
        seen = await panelText();
        return typeof text === "string" ? seen.includes(text) : text.test(seen);
      },
      { timeout, interval: 250 },
    )
    .catch(() => {
      throw new Error(`${PLUGIN} panel never showed ${String(text)}; it shows:\n${seen}`);
    });
}

/** The highest sequence number the panel's log mentions (0 when none). */
async function lastSeq(): Promise<number> {
  await openPanel();
  const seqs = [...(await panelText()).matchAll(/#(\d+)/g)].map((m) => Number(m[1]));
  return Math.max(0, ...seqs);
}

/** Clicks a button of the plugin's panel. */
async function press(label: string): Promise<void> {
  await openPanel();
  const button = $(`//*[contains(@class,'pstrip-body')]//button[normalize-space()='${label}']`);
  await button.waitForClickable({ timeout: 10_000, timeoutMsg: `the panel has no "${label}"` });
  await button.click();
}

/**
 * A presses `label` and waits for a new "<action>: sent #<seq> <size> B"; returns the sequence
 * number, so B can wait for exactly that message. A refused send also takes a number, so the new
 * one is only known to be higher than any before; a refusal fails with the plugin's error name.
 */
async function send(label: string, action: string, size: number): Promise<number> {
  const before = await lastSeq();
  await press(label);
  const line = new RegExp(`${action}: sent #(\\d+) ${String(size)} B`, "g");
  let seq = 0;
  await browser
    .waitUntil(
      async () => {
        const seqs = [...(await panelText()).matchAll(line)].map((m) => Number(m[1]));
        seq = Math.max(0, ...seqs);
        return seq > before;
      },
      { timeout: 30_000, interval: 250 },
    )
    .catch(async () => {
      // The plugin logs a refused send by its host error name; show it instead of only "no sent".
      const results = (await panelText()).match(new RegExp(`^${action}: .*$`, "gm")) ?? [];
      const last = results.at(-1) ?? "nothing logged";
      throw new Error(`${label}: no "${action}: sent" line after #${String(before)}; last result: ${last}`);
    });
  return seq;
}

/** The plugin's relay status line, e.g. `Relay: …SwitchedOff`, whatever prefix Debug prints. */
const status = (name: string) => new RegExp(`^Relay: (\\w+::)?${name}$`, "m");

/** A presses `label` and waits for the refusal `<action>: <error>`. */
async function refused(label: string, action: string, error: string): Promise<void> {
  await press(label);
  await waitPanel(`${action}: ${error}`);
}

/** B waits for A's message `seq` of `size` bytes, decoded intact. */
async function bReceives(seq: number, size: number): Promise<void> {
  await apps.onB(() => waitPanel(`received #${String(seq)} ${String(size)} B intact from Alice`, 45_000));
}

/** Opens the Apps menu and chooses "Start Test Relay" (the click is the plugin's gesture). */
async function startStage(): Promise<void> {
  const button = $("button.chat-apps__button");
  await button.waitForClickable({ timeout: 20_000 });
  await button.click();
  const item = $(
    "//div[contains(@class,'chat-apps__menu')]//button[contains(@class,'appmenu__item')][.//span[contains(@class,'appmenu__text')]/text()[normalize-space()='Start Test Relay']]",
  );
  await item.waitForClickable({ timeout: 20_000, timeoutMsg: "the Apps menu has no Start Test Relay" });
  await item.click();
  await $("div.chat-apps__menu").waitForExist({ reverse: true, timeout: 5_000 });
}

/** Waits until the stage's connection chip reads `text`, starting the activity if none runs. */
async function expectChip(text: string): Promise<void> {
  const chip = $(".relay-chip");
  if (!(await chip.isExisting())) await startStage();
  let seen = "";
  await browser
    .waitUntil(
      async () => {
        seen = (await chip.isExisting()) ? (await chip.getText()).trim() : "";
        return seen === text;
      },
      { timeout: 45_000, interval: 500 },
    )
    .catch(() => {
      throw new Error(`stage chip: want "${text}", got "${seen}"`);
    });
}

/** Settings → Plugins (Installed): the relay card's server and plugin rows. */
async function relayCard(): Promise<{ servers: string; plugins: string }> {
  await openSettingsGroup("Plugins");
  const servers = $("ul.relay-servers");
  await servers.waitForExist({ timeout: 20_000, timeoutMsg: "the relay card has no server list" });
  const read = async (selector: string) => {
    const el = $(selector);
    return (await el.isExisting()) ? (await el.getText()).trim() : "";
  };
  return { servers: await read("ul.relay-servers"), plugins: await read("ul.relay-plugins") };
}

/** Waits until the relay card's server list contains `text`, then closes the settings. */
async function expectServerRow(text: string): Promise<void> {
  let seen = "";
  try {
    await browser.waitUntil(
      async () => {
        seen = (await relayCard()).servers;
        return seen.includes(text);
      },
      { timeout: 45_000, interval: 1000 },
    );
  } catch {
    throw new Error(`relay card servers: want "${text}", got "${seen}"`);
  } finally {
    await closeSettings();
  }
}

async function closeSettings(): Promise<void> {
  await browser.keys("Escape");
  await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
}

/** Flips a settings switch found by its label and waits for the new state. */
async function setSwitch(input: ReturnType<typeof $>, on: boolean): Promise<void> {
  await input.waitForExist({ timeout: 20_000 });
  if ((await input.isSelected()) !== on) await input.click();
  await browser.waitUntil(async () => (await input.isSelected()) === on, {
    timeout: 10_000,
    timeoutMsg: `the switch did not turn ${on ? "on" : "off"}`,
  });
}

/** The per-plugin switch "Test Relay may use the relay" in the relay card. */
async function setPluginRelay(on: boolean): Promise<void> {
  await openSettingsGroup("Plugins");
  await setSwitch($(`input[aria-label='${PLUGIN} may use the relay']`), on);
  await closeSettings();
}

/** The global "Let plugins use a server's relay" switch. */
async function setGlobalRelay(on: boolean): Promise<void> {
  await openSettingsGroup("Plugins");
  await setSwitch(
    $(`//input[@role='switch'][@id=//label[.="Let plugins use a server's relay"]/@for]`),
    on,
  );
  await closeSettings();
}

/** The relay's open sessions (`/v1/status`). */
async function relaySessions(): Promise<number> {
  return Number((await relayenv.relayStatus()).sessions ?? 0);
}

/** The relay container's log (stdout and stderr). */
async function relayLog(): Promise<string> {
  const project = process.env.MUMPEER_PROJECT ?? "mumpeer-testenv";
  const ids = await run("docker", [
    "ps",
    "-q",
    "--filter",
    `label=com.docker.compose.project=${project}`,
    "--filter",
    "label=com.docker.compose.service=mumpeer",
  ]);
  const id = ids.stdout.trim().split("\n")[0];
  if (!id) throw new Error("no running relay container");
  const logs = await run("docker", ["logs", id]);
  return logs.stdout + logs.stderr;
}

/** All of a client's app log files, joined. */
function appLogs(id: app.AppId): string {
  const dir = join(app.profile(id), "data", "app.mumcord.client", "logs");
  if (!existsSync(dir)) return "";
  return readdirSync(dir)
    .map((f) => readFileSync(join(dir, f), "utf8"))
    .join("\n");
}

/** The plugin's own log lines on the current client (Diagnostics, in memory only). */
async function pluginLog(): Promise<string[]> {
  const diag = await invokeOk<{ log: { text: string }[] }>("plugin_diagnostics", { id: ID });
  return diag.log.map((line) => line.text);
}

/** How many `peer-ready` events the plugin on the current client got (its "Peer:" line). */
async function readyCount(): Promise<number> {
  await openPanel();
  const match = /^Peer: .* \((\d+)\)$/m.exec(await panelText());
  return Number(match?.[1] ?? 0);
}

type ReadyCounts = { a: number; b: number };

async function readyCounts(): Promise<ReadyCounts> {
  return { a: await readyCount(), b: await apps.onB(readyCount) };
}

async function waitReady(after: number, who: string): Promise<void> {
  await browser.waitUntil(async () => (await readyCount()) > after, {
    timeout: 60_000,
    interval: 500,
    timeoutMsg: `the plugin on ${who} got no new peer-ready`,
  });
}

/**
 * Waits until both clients see each other and the plugin on A knows Bob as its peer. After a
 * reconnect the old peer stays shown, so with `before` each plugin must also have had a new
 * `peer-ready`: only then do messages pass its checks (FR-6.35), in both directions.
 */
async function bothInChannel(before?: ReadyCounts): Promise<void> {
  await personRow("Bob").waitForDisplayed({ timeout: 60_000, timeoutMsg: "A never saw Bob" });
  await waitPanel("Peer: Bob", 60_000);
  // B drops a channel message until Alice's announce is stored (FR-6.35). A's view of Bob is
  // not that check.
  await apps.onB(() => waitPanel("Peer: Alice", 60_000));
  if (before) {
    await waitReady(before.a, "A");
    await apps.onB(() => waitReady(before.b, "B"));
  }
}

(relayenv.relayAvailable() ? describe : describe.skip)(
  "plugin data through the relay, Bovnar documents (AC-9.18 … 9.21, 9.41, 9.42)",
  function () {
    this.timeout(30 * 60_000);
    this.bail(true);

    let port = 0;

    before(async function () {
      this.timeout(20 * 60_000);
      const fixtures = await pluginenv.startPluginFixtures({ thirdParty: {} });
      const repoUrl = fixtures.thirdPartyUrl ?? "";
      const relay = await relayenv.startRelay("plugin-data");
      port = relay.port;
      expect((await relayenv.relayStatus()).uses).toEqual(expect.arrayContaining(["plugin_data"]));

      await app.restart({ env: pluginenv.appEnv() });
      await setUp("Alice", port, repoUrl);
      await apps.startB({ env: pluginenv.appEnv() });
      await apps.onB(async () => {
        await setUp("Bob", port, repoUrl);
        await openPanel();
      });
      await bothInChannel();
    });

    after(async () => {
      await apps.stopB();
      await pluginenv.stopPluginFixtures();
    });

    it("AC-9.18: the stage chip shows the encrypted relay", async () => {
      await waitPanel(status("Available"), 60_000);
      await expectChip(RELAY_CHIP);
      await apps.onB(() => waitPanel(status("Available"), 60_000));
    });

    it("AC-9.18, AC-9.41: 64 KiB documents reach B through the relay, to the channel and 1:1", async () => {
      // The first channel send starts A's broadcast; it waits there and reaches B once B joins.
      const channel = await send("Channel 64 KiB", "channel-big", BIG);
      await bReceives(channel, BIG);
      const direct = await send("Direct 64 KiB", "direct-big", BIG);
      await bReceives(direct, BIG);
      // A small document without the fallback takes the relay as well.
      const small = await send("Channel small", "channel-small", SMALL);
      await bReceives(small, SMALL);
    });

    it("AC-9.18: the relay sees neither the plugin id nor the payload", async () => {
      const relayState = JSON.stringify(await relayenv.relayStatus());
      const log = await relayLog();
      for (const secret of [ID, PLUGIN, BODY]) {
        expect(relayState).not.toContain(secret);
        expect(log).not.toContain(secret);
      }
    });

    it("AC-9.42: malformed bytes are dropped by B's SDK, the next document arrives", async () => {
      await press("Channel malformed bytes");
      await waitPanel("channel-malformed: sent 300 B");
      // The host delivers the bytes unchanged; B's SDK refuses to decode them.
      await apps.onB(() => waitPanel("received malformed 300 B dropped, bytes unchanged from Alice", 45_000));
      const next = await send("Channel small", "channel-small", SMALL);
      await bReceives(next, SMALL);
      await apps.onB(async () => {
        const lines = await pluginLog();
        // The SDK logs the reason and the size only.
        expect(lines).toEqual(
          expect.arrayContaining(["dropped a Bovnar document: malformed (300 bytes)"]),
        );
        const list = await invokeOk<{ plugins: { id: string; status: { kind: string } }[] }>(
          "plugins_list",
        );
        expect(list.plugins.find((p) => p.id === ID)?.status.kind).toBe("running");
      });
    });

    it("AC-9.42: oversized documents and bytes are refused on A before leaving the machine", async () => {
      // The oversized document takes the next sequence number; B must never get it.
      const oversized = (await lastSeq()) + 1;
      await refused("Channel oversized document", "channel-oversized", "too-large");
      await refused("Channel oversized bytes", "channel-oversized-raw", "too-large");
      // The plugin keeps working after the refusals.
      const after = await send("Channel small", "channel-small", SMALL);
      await bReceives(after, SMALL);
      await apps.onB(async () => {
        const text = await panelText();
        expect(text).not.toContain(`received #${String(oversized)} `);
        expect(text).not.toMatch(/received \d+ B unknown/);
      });
    });

    it("AC-9.18, AC-9.42: no app log carries payload content", () => {
      for (const id of ["a", "b"] as const) {
        const logs = appLogs(id);
        expect(logs).not.toContain(BODY);
      }
    });

    it("AC-9.21: the settings card lists the relay and the data Test Relay used", async () => {
      const card = await relayCard();
      await closeSettings();
      expect(card.servers).toContain("relay allows plugin data");
      expect(card.plugins).toContain(PLUGIN);
      expect(card.plugins).toMatch(/Unofficial · .+ this session/);
    });

    it("AC-9.18: B out of the channel gets nothing; back in, it receives again", async () => {
      const before = await readyCounts();
      const serverId = await apps.onB(() => invokeOk<number>("voice_server_get"));
      await apps.onB(() => invokeOk("disconnect", { serverId }));
      await waitForAbsent(
        "//div[contains(@class,'tree-person')][.//span[contains(@class,'tree-person__name') and text()='Bob']]",
        { timeout: 30_000, timeoutMsg: "Bob's person row remained after disconnecting" },
      );
      // Once A's plugin has seen B leave, nobody in the channel runs it: the send is refused
      // (FR-9.58: the same reach rules as peers.send), and it takes a sequence number B never gets.
      await waitPanel("Peer: none");
      const missed = (await lastSeq()) + 1;
      await refused("Channel 64 KiB", "channel-big", "no-recipients");
      await apps.onB(async () => {
        await invokeOk("connect", { serverId, channelPath: null, password: null });
        await joinVoice();
      });
      // New announces both ways, so A's next send has B as recipient and B accepts it.
      await bothInChannel(before);
      // Back in the channel, B is admitted to the broadcast again with A's next send and gets it.
      const back = await send("Channel 64 KiB", "channel-big", BIG);
      await bReceives(back, BIG);
      await apps.onB(async () => {
        await openPanel();
        expect(await panelText()).not.toContain(`received #${String(missed)} `);
      });
    });

    it("AC-9.20: turning off the plugin's relay permission moves it off the relay at once", async () => {
      await setPluginRelay(false);
      await waitPanel(status("SwitchedOff"));
      // The plugin got `relay-status-changed`.
      await waitPanel(/^relay: (\w+::)?SwitchedOff$/m);
      await refused("Channel 64 KiB", "channel-big", "relay-unavailable");
      // The relay shows A's sessions of this plugin ended (B sent nothing, so none remain).
      await browser.waitUntil(async () => (await relaySessions()) === 0, {
        timeout: 30_000,
        timeoutMsg: "the relay still has plugin data sessions",
      });
      // Small documents still go out through the Mumble fallback.
      const small = await send("Channel small, fallback", "channel-small-fallback", SMALL);
      await bReceives(small, SMALL);
      await setPluginRelay(true);
      await waitPanel(status("Available"), 30_000);
    });

    it("AC-9.20: turning off the global switch does the same", async () => {
      await setGlobalRelay(false);
      await waitPanel(status("SwitchedOff"));
      await refused("Direct 64 KiB", "direct-big", "relay-unavailable");
      await setGlobalRelay(true);
      await waitPanel(status("Available"), 30_000);
      const big = await send("Direct 64 KiB", "direct-big", BIG);
      await bReceives(big, BIG);
    });

    /**
     * AC-9.19 and AC-9.41 without a relay for plugin data: after the switch both clients
     * reconnect to the recreated server (registrations are kept).
     */
    async function withoutRelayData(before: ReadyCounts): Promise<void> {
      // The recreated server drops the connection. Coming back can take longer than a fresh
      // connect, and the old "Peer:" line stays on the panel until the new announce.
      await joinVoice(90_000);
      await apps.onB(() => joinVoice(90_000));
      await bothInChannel(before);
      await waitPanel(status("NotAllowed"), 60_000);
      await expectChip(MUMBLE_CHIP);
      const small = await send("Channel small, fallback", "channel-small-fallback", SMALL);
      await bReceives(small, SMALL);
      const direct = await send("Direct small, fallback", "direct-small-fallback", SMALL);
      await bReceives(direct, SMALL);
      await refused("Channel 64 KiB", "channel-big", "relay-unavailable");
      await refused("Channel 64 KiB, fallback", "channel-big-fallback", "relay-unavailable");
      await refused("Direct 64 KiB", "direct-big", "relay-unavailable");
      await expectServerRow("relay doesn't allow plugin data");
    }

    it("AC-9.19, AC-9.41: profile no-plugin-data falls back to Mumble plugin data", async () => {
      const before = await readyCounts();
      await relayenv.startRelay("no-plugin-data");
      expect((await relayenv.relayStatus()).uses).not.toContain("plugin_data");
      await withoutRelayData(before);
    });

    (oldImage ? it : it.skip)("AC-9.19: an old relay without plugin data falls back the same way", async () => {
      const before = await readyCounts();
      await relayenv.startRelay("old", { image: oldImage, botImage: oldBotImage });
      await withoutRelayData(before);
    });
  },
);
