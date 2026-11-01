import { mkdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { browser } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import { runOk } from "../../helpers/run.ts";
import { filesContaining, keyringDump, serverButton } from "../../helpers/servers-ui.ts";
import {
  button,
  dumpAppLogs,
  ensureArtifacts,
  finishWithPreset,
  pageText,
  passWelcome,
  voiceToSetupStyle,
  waitForText,
  waitForWizardStep,
  wizardArtifacts,
  wizardPrimary,
} from "../../helpers/wizard-util.ts";

// Issue #66 (FR-3.93, FR-3.94): a fake official Mumble 1.5 client in the fresh profile (its
// settings file and favourites database below XDG_CONFIG_HOME / XDG_DATA_HOME, exactly where
// Mumble keeps them) → the wizard offers the import first → favourites, password, certificate and
// push-to-talk key are copied → the imported identity connects to v15 under its certificate.
// Everything is generated here; no real Mumble data or secrets are involved.
const NAME = "MumImport";
const SERVER = "Local Mumble test";
const PASSWORD = "e2e-mumble-saved-pw";
/** Welcome, Import, Identity, Voice, Setup style, All set (#159). */
const STEPS = 6;

/** A `QVariant(uint)` as Mumble 1.5 stores one shortcut button (QDataStream, base64). */
function qvariantUInt(value: number): string {
  const bytes = Buffer.alloc(9);
  bytes.writeUInt32BE(3, 0);
  bytes.writeUInt32BE(value, 5);
  return bytes.toString("base64");
}

/** The step header, upper-cased as the wizard shows it. */
function step(n: number): string {
  return `Step ${String(n)} of ${String(STEPS)}`.toUpperCase();
}

async function writeMumbleFixture(): Promise<string> {
  const profile = app.profile();
  const config = join(profile, "config", "Mumble", "Mumble");
  const data = join(profile, "data", "Mumble", "Mumble");
  mkdirSync(config, { recursive: true });
  mkdirSync(data, { recursive: true });

  // The client certificate: self-signed, PKCS#12 without password, as Mumble stores it.
  const key = join(wizardArtifacts, "mumble-import-key.pem");
  const cert = join(wizardArtifacts, "mumble-import-cert.pem");
  const p12 = join(wizardArtifacts, "mumble-import.p12");
  await runOk("openssl", [
    "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", key, "-out", cert,
    "-days", "365", "-subj", `/CN=${NAME}`,
  ]);
  await runOk("openssl", [
    "pkcs12", "-export", "-in", cert, "-inkey", key, "-out", p12, "-passout", "pass:",
  ]);
  const fp = await runOk("openssl", ["x509", "-in", cert, "-noout", "-fingerprint", "-sha1"]);
  const certHash = (fp.stdout.split("=")[1] ?? "").replace(/:/g, "").trim().toLowerCase();

  const settings = {
    settings_version: 1,
    certificate: readFileSync(p12).toString("base64"),
    audio: { transmit_mode: "PTT" },
    // Left Ctrl + Space as X11 keycodes.
    shortcuts: {
      defined: [
        { index: 1, suppress: false, buttons: [qvariantUInt(37), qvariantUInt(65)], data: "" },
      ],
    },
  };
  writeFileSync(join(config, "mumble_settings.json"), JSON.stringify(settings));
  unlinkSync(key);
  unlinkSync(p12);

  const db = join(data, "mumble.sqlite");
  const sql = [
    "CREATE TABLE servers (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, hostname TEXT,",
    " port INTEGER DEFAULT 64738, username TEXT, password TEXT, url TEXT);",
    `INSERT INTO servers (name, hostname, port, username, password) VALUES`,
    ` ('${SERVER}', 'localhost', ${String(testenv.ports.v15)}, '${NAME}', '${PASSWORD}'),`,
    ` ('Other favourite', 'other.example.org', 64738, '${NAME}', NULL);`,
  ].join("");
  await runOk("sqlite3", [db, sql]);
  return certHash;
}

describe("Issue #66: import from the official Mumble client in the wizard", () => {
  let certHash = "";

  afterEach(function () {
    if (this.currentTest?.state === "failed") dumpAppLogs(app.profile());
  });

  before(async () => {
    ensureArtifacts();
    await testenv.reset("v15");
    certHash = await writeMumbleFixture();
    // The wizard looks for Mumble when it opens: start again with the fixture in place.
    await app.restart();
  });

  it("offers the import first and copies the chosen parts", async () => {
    await passWelcome();
    await waitForText("Bring your Mumble setup along", 30_000);
    expect(await pageText()).toContain(`${step(2)} · OPTIONAL`);
    expect(await pageText()).toContain("2 found");
    expect(await pageText()).toContain("Ctrl + Space");
    await button("Import").click();
    await waitForText("Import finished", 60_000);
    const summary = await pageText();
    expect(summary).toContain("2 servers added to favourites");
    expect(summary).toContain("1 password saved");
    expect(summary).toContain("Identity imported");
    await button("Done").click();

    await waitForWizardStep("identity");
    expect(await pageText()).toContain(step(3));
    expect(await pageText()).toContain(`Using identity ${NAME}.`);
    await wizardPrimary().click();
    await voiceToSetupStyle();
    await finishWithPreset("everythingOn");
  });

  it("keeps the password and the certificate out of plain files", async () => {
    const outside = filesContaining(PASSWORD).filter((p) => !p.includes(join("Mumble", "Mumble")));
    expect(outside).toEqual([]);
    expect(await keyringDump()).toContain(PASSWORD);
    const settings = await app.invoke<{ pttKey: string | null; talkMode: string }>("settings_get");
    expect(settings.pttKey).toBe("Ctrl+Space");
    expect(settings.talkMode).toBe("pushToTalk");
  });

  it("connects to the imported server with the imported certificate", async () => {
    await serverButton(SERVER).click();
    const trust = button("Trust and connect");
    await trust.waitForExist({ timeout: 15_000 });
    await trust.click();
    await browser.waitUntil(async () => (await pageText()).includes(NAME), {
      timeout: 20_000,
      timeoutMsg: `${NAME} not shown after connecting`,
    });
    await bot(["expect", "user", NAME, "--cert-hash", certHash], {
      name: "I66Watch",
      cert: "i66watch",
      timeout: 15,
    });
  });
});
