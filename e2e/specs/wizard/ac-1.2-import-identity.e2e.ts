import { existsSync, unlinkSync } from "node:fs";
import { join } from "node:path";
import { $, browser } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import { runOk } from "../../helpers/run.ts";
import {
  button,
  dumpAppLogs,
  ensureArtifacts,
  finishWizardWithDefaults,
  labelled,
  pageText,
  waitForText,
  wizardArtifacts,
} from "../../helpers/wizard-util.ts";

// AC-1.2: Wizard with an exported PKCS#12 file → import → connecting to the test server shows the
// same registered name as the original identity. The original is a bot identity registered on v15
// first; its cert/key are exported to PKCS#12 with openssl.
const ORIGINAL = "C1Orig";
const CERT = "c1orig";
const P12_PASSWORD = "c1-secret";

function botCertDir(): string {
  const base =
    process.env.MUMCORD_BOT_DATA ??
    join(process.env.XDG_DATA_HOME ?? join(process.env.HOME ?? "", ".local/share"), "mumcord-bot");
  return join(base, "certs", CERT);
}

function sha1Hex(fingerprintLine: string): string {
  return (fingerprintLine.split("=")[1] ?? "").replace(/:/g, "").trim().toLowerCase();
}

describe("AC-1.2 wizard imports a PKCS#12 identity", () => {
  const p12 = join(wizardArtifacts, "c1orig.p12");
  let certHash = "";

  afterEach(function () {
    if (this.currentTest?.state === "failed") dumpAppLogs(app.profile());
  });

  before(async () => {
    ensureArtifacts();
    await testenv.reset("v15");
    // Original identity: register it on v15 under its name.
    // The freshly reset server needs a moment before it accepts TLS; retry the first connect.
    for (let attempt = 1; ; attempt++) {
      try {
        await bot(["register"], { name: ORIGINAL, cert: CERT, timeout: 20 });
        break;
      } catch (error) {
        if (attempt >= 15) throw error;
        await new Promise((r) => setTimeout(r, 2000));
      }
    }
    const dir = botCertDir();
    const pem = join(wizardArtifacts, "c1orig-cert.pem");
    const keyPem = join(wizardArtifacts, "c1orig-key.pem");
    await runOk("openssl", ["x509", "-inform", "DER", "-in", join(dir, "cert.der"), "-out", pem]);
    await runOk("openssl", ["pkey", "-inform", "DER", "-in", join(dir, "key.der"), "-out", keyPem]);
    if (existsSync(p12)) unlinkSync(p12);
    await runOk("openssl", [
      "pkcs12", "-export", "-in", pem, "-inkey", keyPem, "-out", p12,
      "-name", ORIGINAL, "-passout", `pass:${P12_PASSWORD}`,
    ]);
    unlinkSync(keyPem);
    const fp = await runOk("openssl", ["x509", "-in", pem, "-noout", "-fingerprint", "-sha1"]);
    certHash = sha1Hex(fp.stdout);
    console.log(`AC-1.2 original cert hash ${certHash}`);
  });

  it("imports the file in the wizard", async () => {
    await waitForText("Step 1 of 3", 30_000);
    await $('input[name="identity-mode"][value="import"]').click();
    await button("Choose file…").click();
    // Drive the native GTK file chooser: Ctrl+L opens the location field.
    await browser.pause(2000);
    await runOk("xdotool", ["key", "ctrl+l"]);
    await browser.pause(300);
    await runOk("xdotool", ["type", "--delay", "15", p12]);
    await browser.pause(300);
    await runOk("xdotool", ["key", "Return"]);
    await waitForText("Chosen file: c1orig.p12", 10_000);
    const pw = $('input[type="password"]');
    await pw.setValue(P12_PASSWORD);
    await button("Next").click();
    await finishWizardWithDefaults();
  });

  it("connecting to v15 shows the original registered name and cert hash", async () => {
    const input = await labelled("Server address or mumble:// link");
    await input.setValue(`localhost:${String(testenv.ports.v15)}`);
    await button("Add and connect").click();
    const trust = button("Trust and connect");
    await trust.waitForExist({ timeout: 15_000 });
    await trust.click();
    await browser.waitUntil(async () => (await pageText()).includes(ORIGINAL), {
      timeout: 20_000,
      timeoutMsg: `registered name ${ORIGINAL} not shown in the app`,
    });
    // What other users see: the original registered user, with the original certificate.
    await bot(["expect", "user", ORIGINAL, "--registered", "true", "--cert-hash", certHash], {
      name: "C1Watch",
      cert: "c1watch",
      timeout: 15,
    });
  });
});
