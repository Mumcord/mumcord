import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { browser, expect } from "@wdio/globals";
import { app } from "../../helpers/index.ts";
import { artifactsDir, bin } from "../../helpers/paths.ts";
import { run } from "../../helpers/run.ts";
import {
  completeWizard,
  dialogButtonIn,
  openSettingsSection,
  settingsAction,
  settingsDialog,
  settingsTitle,
} from "../../helpers/servers-ui.ts";

// Spec 05, FR-5.64 / FR-5.67 and the desktop half of AC-5.21: "Send to your phone" in the
// identity settings. The QR codes are read from pictures of the dialog, the way a phone camera
// gets them, and opened with the shown code by the same reader the phone app uses
// (crates/store/examples/transfer_read.rs). No server is needed: nothing of a transfer touches
// a network.
const ME = "PhoneQA"; // the identity the wizard creates (its label is its name)
const TITLE = "Send to your phone";
const SERVER = { label: "Home", address: "127.0.0.1", port: 64738, username: "phone-qa" };
const SERVER_PASSWORD = "pw-must-not-travel-7f3a";
const SHORT_VALIDITY_S = 8;

interface IdentityRow {
  label: string;
  certHash: string;
  isDefault: boolean;
}

const shots = join(artifactsDir, "send-to-phone");

function qr() {
  return settingsDialog(TITLE).$('[data-testid="phone-transfer-qr"]');
}

async function checkbox(label: string) {
  const text = settingsDialog(TITLE).$(`.//label[normalize-space()="${label}"]`);
  await text.waitForExist();
  return browser.$(`[id="${(await text.getAttribute("for")) ?? ""}"]`);
}

async function openDialog(): Promise<void> {
  await openSettingsSection("account");
  await settingsAction("Send to your phone…").click();
  await settingsTitle(TITLE).waitForDisplayed();
}

async function showCodes(): Promise<void> {
  await dialogButtonIn(TITLE, "Show codes").click();
  await qr().waitForDisplayed({ timeout: 30_000 });
}

/** The typed code as shown (two groups of four). */
async function shownCode(): Promise<string> {
  return settingsDialog(TITLE)
    .$('[data-testid="phone-transfer-code"] [aria-hidden="true"]')
    .getText();
}

/** Position and count from "Code n of N". */
async function position(): Promise<{ current: number; total: number }> {
  const text = await settingsDialog(TITLE).$('[data-testid="phone-transfer-position"]').getText();
  const match = /^Code (\d+) of (\d+)$/.exec(text);
  if (!match) throw new Error(`unexpected position "${text}"`);
  return { current: Number(match[1]), total: Number(match[2]) };
}

/**
 * A picture of every code of the loop. A picture counts only when the same code was up before
 * and after it was taken.
 */
async function photographAll(): Promise<string[]> {
  const pictures = new Map<number, string>();
  const { total } = await position();
  await browser.waitUntil(
    async () => {
      const before = (await qr().getAttribute("aria-label")) ?? "";
      const file = join(shots, `code-${String(pictures.size)}-${String(Date.now())}.png`);
      await qr().saveScreenshot(file);
      if ((await qr().getAttribute("aria-label")) !== before) return false;
      const match = /^QR code (\d+) of (\d+)$/.exec(before);
      if (!match) throw new Error(`unexpected QR label "${before}"`);
      pictures.set(Number(match[1]), file);
      return pictures.size === total;
    },
    { timeout: 120_000, interval: 50, timeoutMsg: "not every QR code could be photographed" },
  );
  return [...pictures.values()];
}

describe("send to your phone (spec 05, FR-5.64, FR-5.67, AC-5.21 desktop half)", function () {
  this.timeout(300_000);

  before(async () => {
    mkdirSync(shots, { recursive: true });
    await app.restart({ freshProfile: true, env: {} });
    await completeWizard(ME);
    await app.invoke("server_add", {
      input: { ...SERVER, password: { kind: "set", value: SERVER_PASSWORD } },
    });
  });

  it("shows nothing secret before the codes are asked for", async () => {
    await openDialog();
    await expect(await checkbox(`Identity: ${ME} (default)`)).toBeSelected();
    await expect(await checkbox("Server list (1 server)")).toBeSelected();
    await expect(qr()).not.toBeExisting();
    await expect(
      settingsDialog(TITLE).$('[data-testid="phone-transfer-code"]'),
    ).not.toBeExisting();
    await expect(settingsDialog(TITLE)).toHaveText(
      expect.stringContaining("Only show them to your own phone."),
    );
  });

  it("cannot start with nothing selected", async () => {
    const identity = await checkbox(`Identity: ${ME} (default)`);
    const servers = await checkbox("Server list (1 server)");
    await identity.click();
    await servers.click();
    await expect(dialogButtonIn(TITLE, "Show codes")).toBeDisabled();
    await identity.click();
    await servers.click();
    await expect(dialogButtonIn(TITLE, "Show codes")).toBeEnabled();
  });

  it("the shown codes and number open on the phone side: identity and server, no password", async () => {
    await showCodes();
    const code = await shownCode();
    expect(code).toMatch(/^\d{4} \d{4}$/);
    const { total } = await position();
    expect(total).toBeGreaterThan(1);
    await expect(settingsDialog(TITLE)).toHaveText(expect.stringMatching(/Expires in [01]:\d\d/));

    const pictures = await photographAll();
    const digits = code.replace(" ", "");
    const read = await run(bin.transferRead, [digits, ...pictures]);
    if (read.code !== 0) throw new Error(`reading the transfer failed: ${read.stderr}`);

    const [me] = (await app.invoke<IdentityRow[]>("identity_list", {})).filter(
      (identity) => identity.label === ME,
    );
    const lines = read.stdout.trim().split("\n");
    expect(lines).toEqual([
      `identity\t${ME}\t${me.certHash}\tdefault`,
      `server\t${SERVER.label}\t${SERVER.address}\t${String(SERVER.port)}\t${SERVER.username}`,
    ]);
    expect(read.stdout).not.toContain(SERVER_PASSWORD);

    // The number is what protects the codes: the same pictures do not open with another one.
    const wrong = digits === "00000000" ? "00000001" : "00000000";
    const refused = await run(bin.transferRead, [wrong, ...pictures]);
    expect(refused.code).not.toBe(0);
  });

  it("a changed selection gives new codes and a new number", async () => {
    const before = await shownCode();
    await (await checkbox("Server list (1 server)")).click();
    await browser.waitUntil(
      async () => (await qr().isExisting()) && (await shownCode()) !== before,
      { timeout: 30_000, timeoutMsg: "the codes were not replaced" },
    );
    await expect(settingsDialog(TITLE)).toHaveText(
      expect.stringContaining("New codes and a new number. Start again on your phone."),
    );
    const pictures = await photographAll();
    const read = await run(bin.transferRead, [(await shownCode()).replace(" ", ""), ...pictures]);
    if (read.code !== 0) throw new Error(`reading the transfer failed: ${read.stderr}`);
    expect(read.stdout).toContain(`identity\t${ME}\t`);
    expect(read.stdout).not.toContain("server\t");
  });

  it("closing takes the codes away; opening again starts at the selection", async () => {
    await dialogButtonIn(TITLE, "Done").click();
    await settingsTitle(TITLE).waitForExist({ reverse: true });
    await expect(browser.$('[data-testid="phone-transfer-qr"]')).not.toBeExisting();
    await openDialog();
    await expect(qr()).not.toBeExisting();
    await expect(dialogButtonIn(TITLE, "Show codes")).toBeDisplayed();
    await browser.keys("Escape");
    await settingsTitle(TITLE).waitForExist({ reverse: true });
  });

  it("replaces the codes by itself when their time is up", async () => {
    await app.restart({
      env: { MUMCORD_TEST_TRANSFER_VALIDITY_S: String(SHORT_VALIDITY_S) },
    });
    await openDialog();
    await showCodes();
    const first = await shownCode();
    await expect(settingsDialog(TITLE)).toHaveText(expect.stringMatching(/Expires in 0:0\d/));
    await browser.waitUntil(
      async () => (await qr().isExisting()) && (await shownCode()) !== first,
      { timeout: (SHORT_VALIDITY_S + 20) * 1_000, timeoutMsg: "the expired codes stayed" },
    );
    await expect(settingsDialog(TITLE)).toHaveText(
      expect.stringContaining("New codes and a new number. Start again on your phone."),
    );
    // The new transfer is a working one of its own.
    const pictures = await photographAll();
    const read = await run(bin.transferRead, [(await shownCode()).replace(" ", ""), ...pictures]);
    if (read.code !== 0) throw new Error(`reading the transfer failed: ${read.stderr}`);
    expect(read.stdout).toContain(`identity\t${ME}\t`);
    await browser.keys("Escape");
    await settingsTitle(TITLE).waitForExist({ reverse: true });
  });
});
