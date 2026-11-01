// Spec 04 (mumpeer relay), the part that needs no relay: the standard local test servers only,
// no relay bot, no mumpeer. AC-4.1 (no bot: server info says so, no `mumcord.peer` data),
// AC-4.8 (relay only without a relay), AC-4.9 (off), AC-4.10 (per-server override wins),
// AC-4.23 (the five options in the wizard, Settings and the server dialog, by keyboard, with
// screen reader labels, in German too) and AC-4.24 (a refused consent opens as "Relay only").
// Not covered here: the two themes of AC-4.23 (the radios use the shared form styles; theme
// switching itself is covered by themes.e2e.ts), and AC-4.9's "no offer sent" can only be shown
// as "no `mumcord.p2p` row", because a single E2E client has nobody who could ask to watch.
// The relay itself is tested by relay-live.e2e.ts (opt-in).
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { browser, $, expect } from "@wdio/globals";
import { app, repoRoot, testenv } from "../../helpers/index.ts";
import { trustIfAsked } from "../../helpers/chat-ui.ts";
import { channelName, doubleClickRow, ownChannel } from "../../helpers/voice-ui.ts";
import { dialogTab, dialogTitle, fill, serverButton, waitServerStatus } from "../../helpers/servers-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { button, waitForText } from "../../helpers/wizard-util.ts";
import { chooseRadio, focus, pressUntil, tabTo, wizard } from "../app/flows.ts";

const USER = "P2pTester";
const ALPHA = "Alpha";
const BRAVO = "Bravo";

const MODES = ["directRelay", "relayFirst", "directOnly", "relayOnly", "off"] as const;
type Mode = (typeof MODES)[number];

const EN_LABELS: Record<Mode, string> = {
  directRelay: "Direct + relay (direct first)",
  relayFirst: "Direct + relay (always relay, hides my IP)",
  directOnly: "Direct only",
  relayOnly: "Relay only",
  off: "Off",
};

let mainWindow = "";

/** Labels (without the "Recommended" badge) and values of the visible P2P radio group. */
async function p2pOptions(): Promise<{ value: string; label: string; checked: boolean }[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLInputElement>("fieldset.p2p-modes input[type=radio]")).map((input) => {
      const label = input.closest("label");
      const badge = label?.querySelector(".p2p-modes__badge")?.textContent ?? "";
      const text = (label?.textContent ?? "").replace(badge, "").trim();
      return { value: input.value, label: text, checked: input.checked };
    }),
  );
}

/** The accessible name and description of the P2P radio group (what a screen reader reads). */
async function p2pGroupA11y(): Promise<{ name: string; description: string }> {
  return browser.execute(() => {
    const group = document.querySelector("fieldset.p2p-modes");
    const describedBy = group?.getAttribute("aria-describedby") ?? "";
    return {
      name: group?.getAttribute("aria-label") ?? "",
      description: describedBy === "" ? "" : (document.getElementById(describedBy)?.textContent ?? ""),
    };
  });
}

async function checkedMode(): Promise<string> {
  return (await p2pOptions()).find((o) => o.checked)?.value ?? "";
}

/**
 * Tabs into the P2P radio group and moves to `value` with the arrow keys only, in its direction:
 * WebKit does not wrap from the last option to the first.
 */
async function chooseP2pByKeyboard(value: string): Promise<void> {
  const inGroup = () => browser.execute(() => Boolean(document.activeElement?.closest("fieldset.p2p-modes")));
  const start = await tabTo(async (f) => f.tag === "input" && f.type === "radio" && (await inGroup()), "peer-to-peer radio group");
  const values = (await p2pOptions()).map((o) => o.value);
  const key = values.indexOf(value) < values.indexOf(start.value) ? "ArrowUp" : "ArrowDown";
  await pressUntil(key, (f) => f.value === value, `peer-to-peer option ${value}`, 10);
  expect((await focus()).checked).toBe(true);
}

/** Settings → Privacy & Connections, the global mode chosen by keyboard, settings closed again. */
async function setGlobalMode(mode: Mode): Promise<void> {
  await openSettingsGroup("Privacy & Connections");
  await $("fieldset.p2p-modes").waitForDisplayed();
  await chooseP2pByKeyboard(mode);
  await browser.keys(["Escape"]);
  await $("fieldset.p2p-modes").waitForExist({ reverse: true });
}

async function addServer(label: string, svc: "v15" | "v15b"): Promise<void> {
  await $('button[aria-label="Add a server"]').click();
  await dialogTitle("Add a server").waitForDisplayed();
  await fill("Address", "127.0.0.1");
  await fill("Port", String(testenv.ports[svc]));
  await fill("Name in the server bar", label);
  await fill("Your name on this server", USER);
  await button("Add server").click();
  await serverButton(label).waitForDisplayed();
}

async function connect(label: string): Promise<void> {
  await serverButton(label).click();
  await trustIfAsked();
  await waitServerStatus(label, "connected");
  await joinVoice();
}

/**
 * Servers connect read-only; sharing needs the voice server, so join the shown server's channel
 * when its read-only notice offers it.
 */
async function joinVoice(): Promise<void> {
  const join = $(".userpanel-readonly__join");
  if (!(await join.isDisplayed())) return;
  await join.click();
  await $(".userpanel-readonly").waitForExist({ reverse: true, timeout: 15_000 });
}

/**
 * Moves voice to the shown server. The read-only notice's Join button only appears while no server
 * has voice; with voice on another server, double-clicking the own channel takes it over.
 */
async function takeVoiceHere(): Promise<void> {
  await browser.waitUntil(async () => (await ownChannel()) !== "", { timeout: 15_000 });
  await doubleClickRow(channelName(await ownChannel()));
}

/**
 * The reason shown on "Share screen" in the composer's Apps menu, `null` when sharing is
 * possible. Focusing the menu item reads the engine's block again, so wait for `expected`.
 */
async function shareReason(expected: string | null): Promise<void> {
  const apps = $("button.chat-apps__button");
  await apps.waitForClickable({ timeout: 20_000 });
  await apps.click();
  const item = $("//div[contains(@class,'chat-apps__menu')]//*[@role='menuitem'][starts-with(normalize-space(),'Share screen')]");
  await item.waitForDisplayed();
  const read = async () => {
    const reason = item.$(".chat-apps__reason");
    return (await reason.isExisting()) ? (await reason.getText()).trim() : null;
  };
  await browser
    .waitUntil(async () => (await read()) === expected, { timeout: 10_000 })
    .catch(async () => {
      throw new Error(`share reason: want ${JSON.stringify(expected)}, got ${JSON.stringify(await read())}`);
    });
  expect(await item.getAttribute("aria-disabled")).toBe(expected === null ? null : "true");
  await browser.keys(["Escape"]);
}

async function switchToInspector(): Promise<void> {
  const handle = (await browser.getWindowHandles()).find((h) => h !== mainWindow);
  if (!handle) throw new Error("packet inspector window is not open");
  await browser.switchToWindow(handle);
}

/** Data ids (`mumcord.hello` …) and directions of all captured plugin-data rows. */
async function inspectorRows(): Promise<{ kind: string; sent: boolean }[]> {
  await switchToInspector();
  const rows = await browser.execute(() =>
    Array.from(document.querySelectorAll("tr[class*='pi-row']")).map((row) => ({
      kind: Array.from(row.querySelectorAll("code")).map((c) => c.textContent.trim()).join(" "),
      sent: row.className.includes("pi-row-sent"),
    })),
  );
  await browser.switchToWindow(mainWindow);
  return rows;
}

/** The value next to `label` in the open server info panel. */
function infoValue(label: string) {
  return $(`//dt[normalize-space()='${label}']/following-sibling::dd[1]`);
}

describe("spec 04 peer-to-peer mode without a relay (AC-4.1, 4.8–4.10, 4.23, 4.24)", function () {
  this.bail(true);

  before(async () => {
    await testenv.reset("v15");
    await testenv.reset("v15b");
  });

  it("AC-4.23: the wizard shows the five options with explanation, chosen by keyboard", async () => {
    // The peer-to-peer choice is on the Custom path's Connections step (#159).
    await wizard(USER, { style: "custom", onStep: async (step) => {
      if (step !== "connections") return;
      const options = await p2pOptions();
      expect(options.map((o) => o.value)).toEqual([...MODES]);
      expect(options.map((o) => o.label)).toEqual(MODES.map((m) => EN_LABELS[m]));
      expect(await checkedMode()).toBe("directRelay");
      expect(await $("fieldset.p2p-modes .p2p-modes__badge").getText()).toBe("Recommended");
      const a11y = await p2pGroupA11y();
      expect(a11y.name).toBe("Peer-to-peer mode");
      expect(a11y.description).toContain("Direct shows your IP address");
      await chooseP2pByKeyboard("directOnly");
    } });
    await openSettingsGroup("Privacy & Connections");
    await $("fieldset.p2p-modes").waitForDisplayed();
    expect(await checkedMode()).toBe("directOnly");
  });

  it("AC-4.23: Settings → Privacy shows the same five options, labelled for screen readers", async () => {
    const options = await p2pOptions();
    expect(options.map((o) => o.label)).toEqual(MODES.map((m) => EN_LABELS[m]));
    const a11y = await p2pGroupA11y();
    expect(a11y.name).toBe("Peer-to-peer mode");
    expect(a11y.description).toContain("The relay hides it from them");
    await browser.keys(["Escape"]);
  });

  it("opens the packet inspector before connecting (it captures only while open)", async () => {
    mainWindow = await browser.getWindowHandle();
    await addServer(ALPHA, "v15");
    await addServer(BRAVO, "v15b");
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

  it("AC-4.1: without a bot, server info shows Mumbot: No and Mumpeer relay: No", async () => {
    await connect(ALPHA);
    await $("button[aria-label^='Server options']").click();
    await $("//*[@role='menuitem'][normalize-space()='Server info']").click();
    await infoValue("Mumbot").waitForDisplayed({ timeout: 15_000 });
    expect(await infoValue("Mumbot").getText()).toBe("No");
    expect(await infoValue("Mumpeer relay").getText()).toBe("No");
    await browser.keys(["Escape"]);
  });

  it("AC-4.1: nothing about mumcord.peer in the packet inspector", async () => {
    // Several seconds of the session (pings, own state) so a stray announcement would be there.
    await browser.pause(3000);
    const rows = await inspectorRows();
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.filter((r) => r.kind.includes("mumcord.peer"))).toEqual([]);
  });

  it("AC-4.9: in Off, sharing is unavailable with 'Peer-to-peer is off.' and no offer is sent", async () => {
    await setGlobalMode("off");
    await shareReason("Peer-to-peer is off.");
    const rows = await inspectorRows();
    expect(rows.filter((r) => r.sent && r.kind.includes("mumcord.p2p"))).toEqual([]);
  });

  it("AC-4.8: in Relay only on a server without a bot, sharing is unavailable with 'This server has no relay.'", async () => {
    await setGlobalMode("relayOnly");
    await shareReason("This server has no relay.");
  });

  it("AC-4.10 / AC-4.23: the server dialog offers 'Use global setting' plus the five options; the override is saved", async () => {
    await setGlobalMode("directOnly");
    await shareReason(null);
    await browser.execute((name: string) => {
      document.querySelector<HTMLElement>(`ul[aria-label="Saved servers"] button[aria-label^="${name}"]`)?.focus();
    }, ALPHA);
    await browser.keys(["Shift", "F10"]);
    await $("//*[@role='menuitem'][normalize-space()='Edit server']").click();
    await dialogTab("Behaviour");
    await $("fieldset.p2p-modes").waitForDisplayed();
    const options = await p2pOptions();
    expect(options.map((o) => o.value)).toEqual(["global", ...MODES]);
    expect(options[0]?.label).toBe("Use global setting");
    expect(await checkedMode()).toBe("global");
    expect((await p2pGroupA11y()).description).toContain("Only for this server.");
    await chooseP2pByKeyboard("relayOnly");
    await $("dialog[open] .servers-dialog__actions button.button--primary").click();
    await $("fieldset.p2p-modes").waitForExist({ reverse: true, timeout: 15_000 });
  });

  it("AC-4.10: the override wins on its server; the other server follows the global Direct only", async () => {
    // Still connected and the voice server: its label then reads "Alpha (connected), voice server".
    await serverButton(ALPHA).click();
    await joinVoice();
    await shareReason("This server has no relay.");
    // Connecting Bravo leaves voice on Alpha; the share reason follows the voice server.
    await connect(BRAVO);
    await takeVoiceHere();
    await shareReason(null);
    // Back on the overridden server, still the relay rule.
    await serverButton(ALPHA).click();
    await takeVoiceHere();
    await shareReason("This server has no relay.");
  });

  it("AC-4.24: settings saved with allow_direct_connections = no open as Relay only", async () => {
    // Quit, then write a settings record from before the relay existed: consent "no", no mode.
    await browser.deleteSession();
    await app.stopDriver();
    const db = join(app.profile(), "data", "app.mumcord.client", "mumcord.sqlite3");
    execFileSync("sqlite3", [
      db,
      "UPDATE settings SET data = json_remove(json_set(data, '$.allow_direct_connections', 'no'), '$.p2p_mode') WHERE id = 1;",
    ]);
    await app.startDriver();
    await browser.reloadSession();
    mainWindow = await browser.getWindowHandle();
    await serverButton(ALPHA).waitForDisplayed({ timeout: 30_000 });
    await openSettingsGroup("Privacy & Connections");
    await $("fieldset.p2p-modes").waitForDisplayed();
    expect(await checkedMode()).toBe("relayOnly");
  });

  it("AC-4.23: in German the options and their labels are translated", async () => {
    const de = JSON.parse(readFileSync(join(repoRoot, "ui", "src", "locales", "de", "settings.json"), "utf8")) as {
      p2p: { modeLabel: string; explanation: string; mode: Record<Mode, string> };
    };
    await $("//button[@role='tab'][normalize-space()='General']").click();
    await chooseRadio("de");
    await $('[role="tab"][id$="-tab-privacy"]').click();
    await $("fieldset.p2p-modes").waitForDisplayed();
    await browser.waitUntil(async () => (await p2pGroupA11y()).name === de.p2p.modeLabel, {
      timeoutMsg: "the radio group was not translated",
    });
    expect((await p2pOptions()).map((o) => o.label)).toEqual(MODES.map((m) => de.p2p.mode[m]));
    expect((await p2pGroupA11y()).description).toBe(de.p2p.explanation);
    for (const mode of MODES) expect(de.p2p.mode[mode]).not.toBe(EN_LABELS[mode]);
  });
});
