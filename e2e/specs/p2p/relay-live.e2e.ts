// Spec 04 against a running relay: AC-4.2 (the relay bot announces the relay within 2 s of
// joining; server info shows the bot and the relay). Opt-in: runs only with MUMCORD_E2E_RELAY=1,
// so CI (which has no relay) skips it. Never point it at a public server.
//
// How to run (all local, no sudo needed):
//   1. git clone -b feat/relay https://github.com/Mumcord/mumpeer
//   2. git clone -b feat/peer-module https://github.com/Mumcord/mumbot
//      docker build -t mumbot-peer:local mumbot      # until a mumbot release has `mumbot peer`
//   3. MUMBOT_IMAGE=mumbot-peer:local mumpeer/testenv/up.sh
//      (Mumble server on port 44758, relay 44759, relay bot "RelayBot")
//   4. MUMCORD_E2E_RELAY=1 npm run e2e -- --spec e2e/specs/p2p/relay-live.e2e.ts
//   5. mumpeer/testenv/down.sh
// MUMCORD_E2E_RELAY_PORT (default 44758) and MUMCORD_E2E_RELAY_HOST (the relay's announced host,
// default 127.0.0.1) follow MUMPEER_PORT_BASE / MUMPEER_PUBLIC_HOST of the testenv.
//
// Not automated (they need two Mumcord clients, the netns.sh namespaces with sudo, or stopping
// services mid-test); check them by hand with mumpeer/testenv/README.md:
//   AC-4.2 "sent to A only" and AC-4.3 (second / official client), AC-4.4 – AC-4.7 (direct, relay
//   over UDP and TCP 443, relay_first hides the IP), AC-4.11 (bot leaves), AC-4.12 (relay down),
//   AC-4.13 (new relay key → trust prompt), AC-4.14 (two relays / unregistered sender),
//   AC-4.15 – AC-4.18 (relay access rules, API, limits, viewer leaves), AC-4.19 (token rate limit),
//   AC-4.20 (no IPs, hashes or tokens in logs), AC-4.21 (`tok: •••` needs a relayed share between
//   two clients; the masking itself is unit-tested in crates/engine/src/inspector.rs), AC-4.22.
import { browser, $, expect } from "@wdio/globals";
import { trustIfAsked } from "../../helpers/chat-ui.ts";
import { dialogTitle, fill, serverButton, waitServerStatus } from "../../helpers/servers-ui.ts";
import { button, waitForText } from "../../helpers/wizard-util.ts";
import { wizard } from "../app/flows.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";

const enabled = process.env.MUMCORD_E2E_RELAY === "1";
const PORT = Number(process.env.MUMCORD_E2E_RELAY_PORT ?? 44758);
const RELAY_HOST = process.env.MUMCORD_E2E_RELAY_HOST ?? "127.0.0.1";
const USER = "RelayTester";
const SERVER = "Relay test";

interface Row {
  time: string;
  kind: string;
  received: boolean;
}

let mainWindow = "";

async function switchToInspector(): Promise<void> {
  const handle = (await browser.getWindowHandles()).find((h) => h !== mainWindow);
  if (!handle) throw new Error("packet inspector window is not open");
  await browser.switchToWindow(handle);
}

/** All captured rows of the inspector (call with the inspector window active). */
async function rows(): Promise<Row[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll("tr.pi-row")).map((row) => ({
      time: row.querySelector(".pi-time")?.textContent.replace(/[▸▾]/g, "").trim() ?? "",
      kind: row.querySelector("code.pi-kind-id")?.textContent.trim() ?? "",
      received: row.className.includes("pi-row-received"),
    })),
  );
}

/** "hh:mm:ss.mmm" of the inspector as milliseconds since midnight. */
function ms(time: string): number {
  const [h, m, s] = time.split(":").map(Number);
  return (h * 60 + m) * 60_000 + Math.round(s * 1000);
}

function infoValue(label: string) {
  return $(`//dt[normalize-space()='${label}']/following-sibling::dd[1]`);
}

(enabled ? describe : describe.skip)("spec 04 live relay (AC-4.2, opt-in: MUMCORD_E2E_RELAY=1)", function () {
  this.bail(true);

  it("completes the wizard and opens the packet inspector", async () => {
    await wizard(USER);
    mainWindow = await browser.getWindowHandle();
    await openSettingsGroup("About");
    const version = $("//dt[normalize-space()='Version']/following-sibling::dd[1]");
    await browser.waitUntil(async () => /\d+\.\d+|dev~\S+/.test(await version.getText()), { timeoutMsg: "version not loaded" });
    for (let i = 0; i < 7; i++) await version.click();
    await waitForText("Developer tools unlocked");
    await openSettingsGroup("Advanced & Developer");
    await button("Open packet inspector").click();
    await browser.waitUntil(async () => (await browser.getWindowHandles()).length === 2, {
      timeout: 15_000,
      timeoutMsg: "packet inspector window never opened",
    });
    await browser.switchToWindow(mainWindow);
    await browser.keys(["Escape"]);
  });

  it("AC-4.2: the bot's relay announcement arrives within 2 s of joining", async () => {
    await $('button[aria-label="Add a server"]').click();
    await dialogTitle("Add a server").waitForDisplayed();
    await fill("Address", "127.0.0.1");
    await fill("Port", String(PORT));
    await fill("Name in the server bar", SERVER);
    await fill("Your name on this server", USER);
    await button("Add server").click();
    await serverButton(SERVER).click();
    await trustIfAsked();
    await waitServerStatus(SERVER, "connected");

    await switchToInspector();
    const announcement = $("//tr[contains(@class,'pi-row-received')][.//code[normalize-space()='mumcord.peer']]");
    await announcement.waitForExist({ timeout: 10_000, timeoutMsg: "no mumcord.peer announcement received" });
    const all = await rows();
    const peer = all.filter((r) => r.received && r.kind === "mumcord.peer");
    expect(peer).toHaveLength(1);
    // The join is complete with the server's ServerSync; measured on the capture times.
    const sync = all.find((r) => r.received && r.kind === "ServerSync");
    if (!sync) throw new Error(`no received ServerSync row; kinds: ${[...new Set(all.map((r) => r.kind))].join(", ")}`);
    const delay = ms(peer[0]?.time ?? "") - ms(sync.time);
    expect(delay).toBeGreaterThanOrEqual(0);
    expect(delay).toBeLessThanOrEqual(2000);

    await announcement.$("button.pi-expand").click();
    const decoded = $("pre[aria-label='Decoded content']");
    await decoded.waitForDisplayed();
    expect(await decoded.getText()).toMatch(/"t": "relay"/);
    await browser.switchToWindow(mainWindow);
  });

  it("AC-4.2: server info shows the relay bot and the available relay", async () => {
    await $("button[aria-label^='Server options']").click();
    await $("//*[@role='menuitem'][normalize-space()='Server info']").click();
    await infoValue("Mumbot").waitForDisplayed({ timeout: 15_000 });
    await browser.waitUntil(async () => (await infoValue("Mumpeer relay").getText()).startsWith("Available"), {
      timeout: 10_000,
      timeoutMsg: "relay never shown as available",
    });
    expect(await infoValue("Mumbot").getText()).toBe("Yes (RelayBot)");
    expect(await infoValue("Mumpeer relay").getText()).toContain(`Available · ${RELAY_HOST}`);
    await browser.keys(["Escape"]);
  });
});
