import { browser, expect } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import {
  button,
  certFingerprint,
  completeWizard,
  dialogTitle,
  fill,
  filesContaining,
  keyringDump,
  launchWithLink,
  normalizeFingerprint,
  serverButton,
  waitServerStatus,
} from "../../helpers/servers-ui.ts";

// AC-1.4 (add server with password, persists, password only in the keyring), AC-1.5 (trust
// dialog on first use, blocking warning after a certificate change) and AC-1.6 (mumble:// link
// opens, connects and joins "Raid"), against testenv v15.
const LINK = "mumble://localhost/Raid";
const PASSWORD = "qa-ac14-Pw-7f3c9e";
const PW_SERVER = "Password server";
const BOT = { name: "QAWatcher", timeout: 25 };

async function fingerprints(): Promise<string[]> {
  const texts: string[] = [];
  for (const code of await browser.$$(".servers-cert__fingerprints code").getElements()) {
    texts.push(normalizeFingerprint(await code.getText()));
  }
  return texts;
}

async function trustNewCert(): Promise<void> {
  await dialogTitle("Trust this server?").waitForDisplayed({ timeout: 20_000 });
  expect(await fingerprints()).toEqual([await certFingerprint("A")]);
  await button("Trust and connect").click();
}

/** Waits until the recreated v15 container accepts clients again. */
async function serverReachable(): Promise<void> {
  await browser.waitUntil(
    () =>
      bot(["connect"], { name: "QAProbe", timeout: 3 }).then(
        () => true,
        () => false,
      ),
    { timeout: 60_000, interval: 1_000, timeoutMsg: "v15 did not come back" },
  );
}

describe("servers (AC-1.4, AC-1.5, AC-1.6)", () => {
  before(async () => {
    await testenv.reset("v15", { cert: "A" });
  });

  after(async () => {
    // Shared servers: always leave v15 on certificate set A.
    await testenv.up("v15", { cert: "A" });
  });

  it("completes the first-start wizard", async () => {
    await completeWizard("Tester");
  });

  it("AC-1.6: a mumble:// link while running offers to save, connects and joins Raid", async () => {
    // Like a browser handing the link to the registered handler: a second launch forwards it.
    const second = launchWithLink(LINK);
    await new Promise<void>((resolve) => second.once("exit", () => {
        resolve();
      }));
    await dialogTitle("Save this server?").waitForDisplayed({ timeout: 20_000 });
    await button("Save and connect").click();
    await trustNewCert();
    await bot(["expect", "user", "Tester", "--in", "Raid"], BOT);
  });

  it("AC-1.4: adds a server with a password via +", async () => {
    await browser.$('button[aria-label="Add a server"]').click();
    await dialogTitle("Add a server").waitForDisplayed();
    await fill("Address", "127.0.0.1");
    await fill("Port", "64738");
    await fill("Name in the server bar", PW_SERVER);
    await fill("Your name on this server", "Tester2");
    await fill("Server password (optional)", PASSWORD);
    await button("Add server").click();
    await serverButton(PW_SERVER).waitForDisplayed();
  });

  it("AC-1.5: first connection shows the trust dialog with the fingerprint; accept connects", async () => {
    await serverButton(PW_SERVER).click();
    await dialogTitle("Switch server?").waitForDisplayed();
    await button("Disconnect and switch").click();
    await trustNewCert();
    await waitServerStatus(PW_SERVER, "connected");
    await bot(["expect", "user", "Tester2"], BOT);
  });

  it("AC-1.4: after a restart the server is still there; password only in the keyring", async () => {
    await app.restart();
    await serverButton(PW_SERVER).waitForDisplayed({ timeout: 20_000 });
    expect(filesContaining(PASSWORD)).toEqual([]);
    expect(await keyringDump()).toContain(PASSWORD);
  });

  it("AC-1.6: a mumble:// link starts Mumcord, connects and joins Raid", async () => {
    // Quit the WebDriver-controlled instance, then start the app the way the OS would for a link.
    await browser.deleteSession();
    await app.stopDriver();
    const cold = launchWithLink(LINK);
    try {
      await bot(["expect", "user", "Tester", "--in", "Raid"], BOT);
    } finally {
      cold.kill("SIGTERM");
      await new Promise<void>((resolve) => {
        if (cold.exitCode !== null) resolve();
        else
          cold.once("exit", () => {
            resolve();
          });
      });
    }
    await bot(["expect", "user-gone", "Tester"], BOT);
    await app.startDriver();
    await browser.reloadSession();
    await serverButton(PW_SERVER).waitForDisplayed({ timeout: 20_000 });
  });

  it("AC-1.5: a changed server certificate shows a blocking warning", async () => {
    await testenv.up("v15", { cert: "B" });
    await serverReachable();
    await serverButton(PW_SERVER).click();
    await dialogTitle("Warning: the server's certificate changed").waitForDisplayed({
      timeout: 20_000,
    });
    expect(await browser.$('[role="alertdialog"]').isExisting()).toBe(true);
    expect(await fingerprints()).toEqual([await certFingerprint("A"), await certFingerprint("B")]);
    // Blocked: while the warning is open the user is not on the server.
    await bot(["expect", "user-gone", "Tester2"], BOT);
    await button("Don't connect").click();
    // Rejecting ends in a failed connection. Note: the status bar shows the generic
    // "certificate check failed" text, not the dedicated errors.certificateChanged message.
    await browser
      .$("//p[contains(normalize-space(),\"Couldn't connect to Password server\")]")
      .waitForDisplayed({ timeout: 10_000 });
    await waitServerStatus(PW_SERVER, null);
    await bot(["expect", "user-gone", "Tester2"], BOT);
  });
});
