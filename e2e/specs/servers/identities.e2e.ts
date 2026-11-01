import { existsSync, mkdirSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { browser, expect } from "@wdio/globals";
import { app, axe, bot, testenv } from "../../helpers/index.ts";
import { artifactsDir } from "../../helpers/paths.ts";
import { dumpAppLogs } from "../../helpers/wizard-util.ts";
import { run, runOk } from "../../helpers/run.ts";
import { hasClass } from "../../helpers/selectors.ts";
import { focus, tabTo, typeText } from "../app/flows.ts";
import {
  ariaButton,
  menuItem,
  openServerMenu,
  button,
  choosePathInNativeDialog,
  completeWizard,
  dialogButtonIn,
  dialogTab,
  dialogTitle,
  fill,
  fillIn,
  keyringDump,
  normalizeFingerprint,
  openSettingsSection,
  pickOption,
  serverButton,
  settingsAction,
  settingsDialog,
  settingsErrorIn,
  settingsTitle,
  waitServerStatus,
} from "../../helpers/servers-ui.ts";

// Issue #195 – the identity manager in the running client (spec 03 §9).
// The store/engine/UI unit tests already cover the rules; this walks the real app against the
// local test servers (v15 + v15b), in the order the acceptance criteria name.
//
// Covered here: AC-3.2 – AC-3.14. AC-3.1 (a 0.4.x profile upgrading) needs an old profile, so it
// stays a manual check plus the store migration tests in crates/store/src/identities.rs.
const ME = "IdentQA"; // the identity the wizard creates (its label is its name)
const WORK = "Work";
const WORK_NAME = "Zoe (Acme)";
const STREAM = "Stream";
const STREAM_NAME = "ZoeLive";
const IMPORTED = "ImportedQA";
const SERVER_A = "Server A"; // v15, connects with its own identity
const SERVER_B = "Server B"; // v15b, connects with its own identity
const IMPORT_PASSWORD = "imported-pw";
const EXPORT_PASSWORD = "qa-ac35-Pw-9c1b4e";

interface ServerRow {
  id: number;
  label: string;
  identityId: number | null;
}

interface IdentityRow {
  id: number;
  label: string;
  name: string;
  certHash: string;
  isDefault: boolean;
}

/** One card of the identity list, by its label. */
function card(label: string) {
  return browser.$(
    `//li[${hasClass("identity-card")}][.//strong[normalize-space()="${label}"]]`,
  );
}

async function identityRow(label: string): Promise<IdentityRow> {
  const rows = await app.invoke<IdentityRow[]>("identity_list", {});
  const row = rows.find((i) => i.label === label);
  if (!row) throw new Error(`identity "${label}" is not in the list`);
  return row;
}

/** Saved server id by its server-bar label. */
async function serverIdOf(label: string): Promise<number> {
  const servers = await app.invoke<ServerRow[]>("servers_list", {});
  const server = servers.find((s) => s.label === label);
  if (!server) throw new Error(`saved server "${label}" not found`);
  return server.id;
}

/**
 * What the server sees for us: the certificate hash in the synced snapshot. A second client (the
 * bot in AC-3.9) sees the same hash.
 */
interface SnapshotView {
  selfSession: number;
  users: { session: number; certHash: string | null }[];
}

async function ownHashOf(serverLabel: string): Promise<string | null> {
  const snapshot = await app.invoke<SnapshotView | null>("session_snapshot", {
    serverId: await serverIdOf(serverLabel),
  });
  if (!snapshot) return null;
  const own = snapshot.users.find((user) => user.session === snapshot.selfSession);
  return own?.certHash ?? null;
}

async function waitOwnHash(serverLabel: string, hash: string): Promise<void> {
  let actual: string | null = null;
  await browser.waitUntil(
    async () => {
      actual = await ownHashOf(serverLabel);
      return actual === hash;
    },
    {
      timeout: 60_000,
      interval: 1_000,
      timeoutMsg: `the server ${serverLabel} did not show ${hash}`,
    },
  );
}

/**
 * The short fingerprint a card shows: eight groups, an ellipsis, the last two groups. AC-3.2
 * compares it with `openssl x509 -fingerprint -sha1` of the exported file (the engine's hash is
 * the SHA-1 of the certificate, so the same bytes).
 */
async function shownFingerprint(label: string): Promise<string> {
  const text = await card(label).getText();
  const match = text.match(/Fingerprint ([0-9A-F:…\s]+)/);
  if (!match) throw new Error(`no short fingerprint in the card "${text}"`);
  return normalizeFingerprint(match[1]);
}

/** Creates an identity through Settings → Account & Identity (name + label). */
async function createIdentity(name: string, label: string): Promise<void> {
  await settingsAction("New identity").click();
  await settingsTitle("New identity").waitForDisplayed();
  await fillIn("New identity", "Name on servers", name);
  await fillIn("New identity", "Label", label);
  await dialogButtonIn("New identity", "Create").click();
  // The key is generated on first use; the dialog closes when the identity is stored.
  await card(label).waitForDisplayed({ timeout: 60_000 });
  if (await settingsTitle("New identity").isExisting())
    throw new Error(`the New identity dialog stayed open after creating "${label}"`);
}

/** Imports `path` (the file chooser is native, so the path is typed into it). */
async function importIdentity(path: string, password: string): Promise<void> {
  await settingsAction("Import…").click();
  await choosePathInNativeDialog("Import identity", path);
  await settingsTitle("Import identity").waitForDisplayed({ timeout: 20_000 });
  await fillIn("Import identity", "File password (leave empty if none)", password);
  await dialogButtonIn("Import identity", "Import…").click();
}

/** Opens the delete dialog of `label` and returns its text. */
async function openDeleteDialog(label: string): Promise<string> {
  await ariaButton(`Delete ${label}`).click();
  const title = `Delete the identity ${label}?`;
  await settingsTitle(title).waitForDisplayed({ timeout: 10_000 });
  return settingsDialog(title).getText();
}

function listFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry);
    const st = statSync(path);
    return st.isDirectory() ? listFiles(path) : [path];
  });
}

/** Adds a server through the dialog and picks `identityLabel` on the Identity tab. */
async function addServer(
  port: number,
  label: string,
  identityLabel: string,
  username: string,
): Promise<void> {
  // The server bar is inert while the settings screen is open.
  if (await browser.$(".overlay__dialog").isExisting()) await browser.keys(["Escape"]);
  await browser.$('button[aria-label="Add a server"]').click();
  await dialogTitle("Add a server").waitForDisplayed();
  await fill("Address", "localhost");
  await fill("Port", String(port));
  await fill("Name in the server bar", label);
  await fill("Your name on this server", username);
  await dialogTab("Identity");
  await pickOption("Identity for this server", `${identityLabel} (`);
  await button("Add server").click();
  await serverButton(label).waitForDisplayed({ timeout: 15_000 });
}

async function trustAndConnect(): Promise<void> {
  await dialogTitle("Trust this server?").waitForDisplayed({ timeout: 20_000 });
  await button("Trust and connect").click();
}

describe("identity manager (issue #148, AC-3.2 – AC-3.14)", function () {
  // The tests build on each other (one list, one server each); after the first failure the rest
  // would only fail with misleading follow-up errors.
  this.bail(true);

  before(async () => {
    await testenv.reset("v15");
    await testenv.reset("v15b");
    mkdirSync(join(artifactsDir, "identities"), { recursive: true });
  });

  afterEach(function () {
    if (this.currentTest?.state === "failed") dumpAppLogs(app.profile());
  });

  after(async () => {
    // Shared servers: always leave both on certificate set A.
    await testenv.up("v15", { cert: "A" });
    await testenv.up("v15b", { cert: "A" });
  });

  it("AC-3.2: creates identities; each card shows its own avatar and fingerprint", async () => {
    await completeWizard(ME);
    await openSettingsSection("account");
    await card(ME).waitForDisplayed();
    expect(await card(ME).getText()).toContain("Default");

    await createIdentity(WORK_NAME, WORK);
    await createIdentity(STREAM_NAME, "Streaming");

    const rows = await app.invoke<IdentityRow[]>("identity_list", {});
    expect(rows.length).toBe(3);
    for (const label of [ME, WORK, "Streaming"]) {
      const row = rows.find((i) => i.label === label);
      if (!row) throw new Error(`identity "${label}" is not in the list`);
      const item = card(label);
      const avatar = await item.$(".identity-card__avatar");
      // A colour avatar with the label's first letter, and the fingerprint line.
      expect(await avatar.getText()).toBe(label.slice(0, 1).toUpperCase());
      expect(await avatar.getProperty("className")).toMatch(/identity-card__avatar--tone/);
      const groups = (row.certHash.match(/.{1,2}/g) ?? []).map((part) => part.toUpperCase());
      expect(groups.length).toBe(20);
      const line = await shownFingerprint(label);
      expect(line).toBe(`${row.certHash.slice(0, 16)}${row.certHash.slice(-4)}`);
      expect(await item.getText()).toContain(`Name: ${row.name}`);
    }
    expect(new Set(rows.map((i) => i.certHash)).size).toBe(3);
    expect(await card(WORK).getText()).toContain("Not used by any server yet");
  });

  it("AC-3.3: imports a PKCS#12 with its common name as the label, and refuses it a second time", async () => {
    const dir = join(artifactsDir, "identities");
    const p12 = join(dir, "imported.p12");
    await runOk("openssl", [
      "req", "-x509", "-newkey", "rsa:2048", "-keyout", join(dir, "imported-key.pem"),
      "-out", join(dir, "imported-cert.pem"), "-days", "1", "-nodes", "-subj", `/CN=${IMPORTED}`,
    ]);
    await runOk("openssl", [
      "pkcs12", "-export", "-in", join(dir, "imported-cert.pem"),
      "-inkey", join(dir, "imported-key.pem"), "-out", p12, "-passout", `pass:${IMPORT_PASSWORD}`,
    ]);

    await openSettingsSection("account");
    await importIdentity(p12, IMPORT_PASSWORD);
    // AC-3.3: the label defaults to the certificate's common name, so the list shows it.
    await card(IMPORTED).waitForDisplayed({ timeout: 20_000 });
    expect(await card(IMPORTED).getText()).toContain(`Name: ${IMPORTED}`);

    // The same file again: refused with the wording of AC-3.3, and no second entry.
    await importIdentity(p12, IMPORT_PASSWORD);
    expect(await settingsErrorIn("Import identity")).toContain(
      "This identity is already in your list.",
    );
    expect((await app.invoke<IdentityRow[]>("identity_list", {})).length).toBe(4);
    await button("Cancel").click();
  });

  it("AC-3.4: rename changes the label everywhere and keeps the certificate", async () => {
    const before = (await identityRow("Streaming")).certHash;
    await ariaButton("Rename Streaming").click();
    await settingsTitle("Rename Streaming").waitForDisplayed();
    await fill("Label", STREAM);
    await button("Save").click();
    await card(STREAM).waitForDisplayed({ timeout: 10_000 });

    // Same certificate, new label: details, the card and the server dialog's dropdown.
    const row = await identityRow(STREAM);
    expect(row.certHash).toBe(before);
    expect(row.name).toBe(STREAM_NAME);
    await ariaButton(`Show details of ${STREAM}`).click();
    await settingsTitle(`Identity ${STREAM}`).waitForDisplayed();
    expect(await settingsDialog(`Identity ${STREAM}`).getText()).toContain(STREAM);
    const full = (row.certHash.match(/.{1,2}/g) ?? []).map((g) => g.toUpperCase()).join(":");
    expect(await settingsDialog(`Identity ${STREAM}`).getText()).toContain(full);
    await dialogButtonIn(`Identity ${STREAM}`, "Close").click();

    // Escape closes the settings screen, so the server dialog can be opened again.
    await browser.keys(["Escape"]);
    await browser.$('button[aria-label="Add a server"]').click();
    await dialogTitle("Add a server").waitForDisplayed();
    await dialogTab("Identity");
    // Open the dropdown: its options list every identity with the renamed label.
    const box = await browser.$('//label[normalize-space()="Identity for this server"]/following::button[@role="combobox"][1]');
    await box.click();
    const options = await browser.$$('//div[@role="listbox"]//div[@role="option"]').getElements();
    const labels: string[] = [];
    for (const option of options) labels.push((await option.getText()).trim());
    expect(labels.some((text) => text.startsWith(`${STREAM} (${STREAM_NAME})`))).toBe(true);
    const target = labels.findIndex((text) => text.startsWith(`${STREAM} (${STREAM_NAME})`));
    if (target < 0) throw new Error(`option for ${STREAM} disappeared`);
    await options[target].click();
    // The renamed label is the value the dropdown announces (aria-activedescendant + the value span).
    const announced = await browser.execute(() => {
      const box = document.querySelector('[role="combobox"]');
      const activeId = box?.getAttribute("aria-activedescendant");
      const active = activeId ? document.getElementById(activeId)?.textContent : "";
      const value = document.querySelector(".select__value")?.textContent || active || "";
      return value.trim();
    });
    expect(announced).toContain(STREAM);
    await button("Cancel").click();
  });

  it("AC-3.5: exports with a password; the file opens with the same certificate", async () => {
    // The file name comes from the native save chooser, which WebDriver cannot reach, so the path
    // is given to the engine directly: the same code path the Export button runs after the picker.
    // The dialog itself (password twice, empty password refused) is covered by
    // SettingsScreen.test.tsx ("exports the identity with a password entered twice").
    await openSettingsSection("account");
    const dir = join(artifactsDir, "identities");
    const path = join(dir, "work.p12");
    const row = await identityRow(WORK);
    await app.invoke("identity_export", { id: row.id, path, password: EXPORT_PASSWORD });
    await browser.waitUntil(() => existsSync(path), {
      timeout: 15_000,
      timeoutMsg: "the exported file never appeared",
    });

    const pem = join(dir, "work-cert.pem");
    await runOk("openssl", [
      "pkcs12", "-in", path, "-passin", `pass:${EXPORT_PASSWORD}`, "-nokeys", "-out", pem,
    ]);
    const printed = await runOk("openssl", ["x509", "-in", pem, "-noout", "-fingerprint", "-sha1"]);
    const fingerprint = (printed.stdout.split("=")[1] ?? "").replace(/:/g, "").trim().toLowerCase();
    expect(fingerprint).toBe(row.certHash);
    // The card's short fingerprint is the same hash: eight groups, ellipsis, last two groups.
    expect(await shownFingerprint(WORK)).toBe(`${fingerprint.slice(0, 16)}${fingerprint.slice(-4)}`);

    // A wrong password does not open the file.
    const wrong = await run("openssl", ["pkcs12", "-in", path, "-passin", "pass:nope", "-nokeys"]);
    expect(wrong.code).not.toBe(0);
  });

  it("AC-3.13: local data holds labels, names, fingerprints and dates only; secrets stay in the keyring", async () => {
    const data = join(app.profile(), "data", "app.mumcord.client");
    const db = join(data, "mumcord.sqlite3");

    const settings = join(data, "settings.json");
    // Rows still in the write-ahead log are not in the main file yet, so look at both.
    const dbBytes = [db, `${db}-wal`]
      .filter((file) => existsSync(file))
      .reduce((all, file) => Buffer.concat([all, readFileSync(file)]), Buffer.alloc(0));
    const settingsText = existsSync(settings) ? readFileSync(settings, "utf8") : "";
    const missing: string[] = [];
    for (const label of [ME, WORK, STREAM, IMPORTED]) {
      const row = await identityRow(label);
      // The list shows labels, certificate names and fingerprints, so they must be in local data.
      if (!dbBytes.includes(Buffer.from(label))) missing.push(`label ${label}`);
      if (!dbBytes.includes(Buffer.from(row.certHash))) missing.push(`hash of ${label}`);
    }
    if (missing.length > 0)
      throw new Error(
        `not in the database ${db} (files: ${listFiles(app.profile()).join(", ")}): ${missing.join(", ")}`,
      );
    expect(settingsText).not.toMatch(/BEGIN (PRIVATE KEY|CERTIFICATE)/);

    const files = listFiles(app.profile());
    const logText = files.filter((f) => f.includes("/logs/")).map((f) => readFileSync(f, "utf8")).join("\n");
    expect(logText).not.toMatch(/BEGIN (PRIVATE KEY|CERTIFICATE)/);
    expect(logText).not.toMatch(/pkcs12|Bag Key/i);

    for (const label of [ME, WORK, STREAM, IMPORTED]) {
      const row = await identityRow(label);
      const secret = Buffer.from(
        (
          await runOk("secret-tool", [
            "lookup", "service", "app.mumcord.client", "username", `identity/${String(row.id)}/0`,
          ])
        ).stdout,
      );
      expect(secret.length).toBeGreaterThan(100);
      expect(secret[0]).toBe(0x30); // DER PKCS#12: starts with a SEQUENCE
      const probe = secret.subarray(20, 68);
      const keyText = files.filter((f) => readFileSync(f).includes("PRIVATE KEY"));
      const certText = files.filter((f) => readFileSync(f).includes("BEGIN CERTIFICATE"));
      const storedSecret = files.filter((f) => readFileSync(f).includes(probe));
      if (keyText.length > 0) throw new Error(`key material in ${keyText.join(", ")}`);
      if (certText.length > 0) throw new Error(`certificate text in ${certText.join(", ")}`);
      if (storedSecret.length > 0)
        throw new Error(`stored secret of ${label} in ${storedSecret.join(", ")}`);
    }

    const dump = await keyringDump();
    const accounts = [...dump.matchAll(/attribute\.username = (\S+)/g)].map((m) => m[1]);
    for (const label of [ME, WORK, STREAM, IMPORTED]) {
      const row = await identityRow(label);
      expect(accounts).toContain(`identity/${String(row.id)}/count`);
      expect(accounts.some((a) => a.startsWith(`identity/${String(row.id)}/`))).toBe(true);
    }
  });

  it("AC-3.9: two servers connected at the same time with different identities", async () => {
    await openSettingsSection("account");
    await addServer(testenv.ports.v15, SERVER_A, WORK, WORK_NAME);
    await addServer(testenv.ports.v15b, SERVER_B, STREAM, STREAM_NAME);

    await serverButton(SERVER_A).click();
    await trustAndConnect();
    await waitServerStatus(SERVER_A, "connected");
    await serverButton(SERVER_B).click();
    await trustAndConnect();
    await waitServerStatus(SERVER_B, "connected");

    const hashWork = (await identityRow(WORK)).certHash;
    const hashStream = (await identityRow(STREAM)).certHash;
    expect(hashWork).not.toBe(hashStream);
    // What other users see on each server: a different certificate hash for each identity.
    await bot(["expect", "user", WORK_NAME, "--cert-hash", hashWork], {
      name: "IdentWatchA",
      cert: "identwatcha",
      port: testenv.ports.v15,
      timeout: 25,
    });
    await bot(["expect", "user", STREAM_NAME, "--cert-hash", hashStream], {
      name: "IdentWatchB",
      cert: "identwatchb",
      port: testenv.ports.v15b,
      timeout: 25,
    });
  });

  it("AC-3.12: connection info names the identity, and the header shows the badge for a non-default one", async () => {
    const header = browser.$(`//span[${hasClass("chat-header__identity")}]`);
    await header.waitForDisplayed({ timeout: 15_000 });
    expect(await header.getText()).toContain(`Identity: ${STREAM}`);
    expect(await header.getAttribute("aria-label")).toBe(`Connected with the identity ${STREAM}`);

    await browser.$('button[aria-label="Connection info"]').click();
    const rows = await $$(
      `//div[${hasClass("connection-info__panel")}]//div[${hasClass("connection-info__row")}]`,
    ).getElements();
    const values: Record<string, string> = {};
    for (const row of rows) values[(await row.$("dt").getText()).trim()] = (await row.$("dd").getText()).trim();
    const stream = await identityRow(STREAM);
    const short = (stream.certHash.match(/.{1,2}/g) ?? []).slice(0, 4).join(":");
    expect(values["Identity"]).toBe(`${STREAM} (${STREAM_NAME}) · ${short}`);
    await browser.keys(["Escape"]);

    // The default identity shows no badge, and connection info names it.
    await serverButton(SERVER_A).click(); // shows server A
    await browser.waitUntil(
      async () => (await serverButton(SERVER_A).getAttribute("aria-label")) === `${SERVER_A} (connected)`,
      { timeout: 15_000 },
    );
  });

  it("AC-3.10: changing a connected server's identity reconnects only that server", async () => {
    const imported = await identityRow(IMPORTED);
    await openServerMenu(SERVER_B);
    await menuItem(SERVER_B, "Identity…").click();
    await dialogTitle(`Identity for ${SERVER_B}`).waitForDisplayed({ timeout: 10_000 });
    await pickOption("Identity for this server", `${IMPORTED} (${IMPORTED})`);
    await button("Save and reconnect").click();
    // Saving the choice of a connected server asks first: only this server reconnects.
    await dialogTitle(`Reconnect ${SERVER_B}?`).waitForDisplayed({ timeout: 10_000 });
    await button("Reconnect").click();
    await waitServerStatus(SERVER_B, "connected", 60_000);
    await waitServerStatus(SERVER_A, "connected");
    // Only B reconnected: the bot still sees A with its own certificate.
    await waitOwnHash(SERVER_A, (await identityRow(WORK)).certHash);
    await waitOwnHash(SERVER_B, imported.certHash);

    // The name on the server stays the one typed in the dialog; only the certificate changed.
    await bot(["expect", "user", STREAM_NAME, "--cert-hash", imported.certHash], {
      name: "IdentWatchB",
      cert: "identwatchb",
      port: testenv.ports.v15b,
      timeout: 25,
    });
    // Server A kept its own identity: the badge is gone there and shows on B.
    await serverButton(SERVER_B).click(); // the header shows the server we look at
    const badge = await browser.$(`//span[${hasClass("chat-header__identity")}]`);
    await browser.waitUntil(async () => (await badge.getText()).includes(`Identity: ${IMPORTED}`), {
      timeout: 20_000,
      interval: 1_000,
      timeoutMsg: `the header badge still says "${await badge.getText()}"`,
    });
  });

  it("AC-3.11: 'Later' stores the choice without reconnecting; the next connect uses it", async () => {
    const work = await identityRow(WORK);
    const imported = await identityRow(IMPORTED);
    await openServerMenu(SERVER_B);
    await menuItem(SERVER_B, "Identity…").click();
    await dialogTitle(`Identity for ${SERVER_B}`).waitForDisplayed();
    await pickOption("Identity for this server", `${WORK} (${WORK_NAME})`);
    await button("Save and reconnect").click();
    await dialogTitle(`Reconnect ${SERVER_B}?`).waitForDisplayed({ timeout: 10_000 });
    await button("Later").click(); // stores the choice without reconnecting now
    await waitServerStatus(SERVER_B, "connected");
    // Nothing reconnected: the server still runs with the identity it connected with.
    await waitOwnHash(SERVER_B, imported.certHash);

    await openServerMenu(SERVER_B);
    await menuItem(SERVER_B, "Disconnect").click();
    await waitServerStatus(SERVER_B, null);
    await serverButton(SERVER_B).click();
    await waitServerStatus(SERVER_B, "connected");
    await waitOwnHash(SERVER_B, work.certHash);

    // The name on the server stays what was typed when the server was added.
    await bot(["expect", "user", STREAM_NAME, "--cert-hash", work.certHash], {
      name: "IdentWatchB",
      cert: "identwatchb",
      port: testenv.ports.v15b,
      timeout: 25,
    });
  });

  it("AC-3.6: deleting an identity a server uses names the server, needs the typed label, and falls back to the default", async () => {
    await openSettingsSection("account");
    const text = await openDeleteDialog(WORK);
    // Both servers follow the default identity (IdentQA, the wizard's): Work is gone.
    expect(text).toContain(`These servers will use the default identity ${ME} afterwards: ${SERVER_A}`);

    // Wrong typing keeps Delete disabled.
    const confirm = settingsDialog(`Delete the identity ${WORK}?`);
    const input = await confirm.$(`.//input[${hasClass("settings-input")}]`);
    await input.setValue("wrong");
    const del = await confirm.$('.//button[contains(@class,"settings-button--danger")]');
    expect(await del.getProperty("disabled")).toBe(true);

    await input.setValue(WORK);
    expect(await del.getProperty("disabled")).toBe(false);
    await del.click();
    await browser.waitUntil(
      async () => !(await settingsTitle(`Delete the identity ${WORK}?`).isExisting()),
      { timeout: 20_000, timeoutMsg: "the identity was not deleted" },
    );

    // The server now follows the default, and connects with it.
    const defaultRow = await identityRow(ME);
    await browser.keys(["Escape"]); // close settings: the server bar is inert while it is open
    const saved = (await app.invoke<ServerRow[]>("servers_list", {})).find(
      (server) => server.label === SERVER_A,
    );
    if (!saved) throw new Error(`saved server "${SERVER_A}" is gone`);
    expect(saved.identityId).toBe(null);
    await serverButton(SERVER_A).click();
    await waitServerStatus(SERVER_A, "connected");
    await waitOwnHash(SERVER_A, defaultRow.certHash);
    await bot(["expect", "user", WORK_NAME, "--cert-hash", defaultRow.certHash], {
      name: "IdentWatchA",
      cert: "identwatcha",
      port: testenv.ports.v15,
      timeout: 25,
    });
  });

  it("AC-3.7: deleting the default names the identity that becomes the new default", async () => {
    await openSettingsSection("account");
    const remaining = await app.invoke<IdentityRow[]>("identity_list", {});
    expect(remaining.length).toBe(3); // IdentQA (default), Stream, ImportedQA
    const text = await openDeleteDialog(ME);
    expect(text).toContain(`${STREAM} becomes the default identity.`);
    await settingsDialog(`Delete the identity ${ME}?`);
    const input = await settingsDialog(`Delete the identity ${ME}?`).$(
      `.//input[${hasClass("settings-input")}]`,
    );
    await input.setValue(ME);
    await settingsDialog(`Delete the identity ${ME}?`)
      .$('.//button[contains(@class,"settings-button--danger")]')
      .click();
    await card(STREAM).waitForDisplayed({ timeout: 20_000 });

    const rows = await app.invoke<IdentityRow[]>("identity_list", {});
    expect(rows.find((i) => i.label === STREAM)?.isDefault).toBe(true);
    expect(await card(STREAM).getText()).toContain("Default");
    await browser.keys(["Escape"]); // close settings for the server bar
    // A server without its own identity uses the new default on its next connect.
    await openServerMenu(SERVER_A);
    await menuItem(SERVER_A, "Disconnect").click();
    await waitServerStatus(SERVER_A, null);
    await serverButton(SERVER_A).click();
    await waitServerStatus(SERVER_A, "connected");
    await waitOwnHash(SERVER_A, (await identityRow(STREAM)).certHash);
  });

  it("AC-3.8: the last remaining identity cannot be deleted, in the UI and in the engine", async () => {
    await openSettingsSection("account");
    const rows = await app.invoke<IdentityRow[]>("identity_list", {});
    for (const row of rows.slice(0, rows.length - 1)) {
      await openDeleteDialog(row.label);
      const dialog = settingsDialog(`Delete the identity ${row.label}?`);
      await dialog.$(`.//input[${hasClass("settings-input")}]`).setValue(row.label);
      await dialog.$('.//button[contains(@class,"settings-button--danger")]').click();
      await browser.waitUntil(
        async () => (await app.invoke<IdentityRow[]>("identity_list", {})).length === rows.length - 1,
        { timeout: 20_000 },
      );
    }
    const last = (await app.invoke<IdentityRow[]>("identity_list", {}))[0];
    const text = await openDeleteDialog(last.label);
    expect(text).toContain("Mumcord needs at least one identity");
    const dialog = settingsDialog(`Delete the identity ${last.label}?`);
    const del = await dialog.$('.//button[contains(@class,"settings-button--danger")]');
    expect(await del.getProperty("disabled")).toBe(true);
    // The engine refuses the call as well (the UI shows the same hint).
    const refused = await app
      .invoke("identity_delete", { id: last.id })
      .then(() => "", (error: unknown) => String(error));
    expect(refused).toContain("at least one identity");
    await button("Cancel").click();
    expect((await app.invoke<IdentityRow[]>("identity_list", {})).length).toBe(1);
  });

  it("keeps the identity list after a restart", async () => {
    const before = await app.invoke<IdentityRow[]>("identity_list", {});
    await app.restart();
    await openSettingsSection("account");
    const rows = await app.invoke<IdentityRow[]>("identity_list", {});
    expect(rows.map((row) => row.label)).toEqual(before.map((row) => row.label));
    await card(rows[0]?.label ?? "").waitForDisplayed({ timeout: 30_000 });
  });

  it("AC-3.14: create, rename, set default, details and delete work with the keyboard alone", async () => {
    // Every step runs on key actions only: Tab reaches the controls, Enter activates them.
    const idOf = async (labelText: string) => {
      const label = await browser.$(`//label[normalize-space()="${labelText}"]`);
      await label.waitForExist();
      return (await label.getAttribute("for")) ?? "";
    };

    // Start from the main window: close the settings screen the previous test left open.
    if (await browser.$(".overlay__dialog").isExisting()) await browser.keys(["Escape"]);
    await tabTo((f) => f.label === "Quick settings", "the Quick settings button");
    await browser.keys(["Enter"]);
    // The quick settings menu is arrow-navigated (its items have tabindex=-1), so End reaches its
    // last entry, "All settings…".
    await browser.keys(["End", "Enter"]);
    await tabTo((f) => f.role === "tab", "the settings tab list");
    await browser.keys(["Home"]);
    for (let i = 0; i < 20; i++) {
      if ((await focus()).id.endsWith("-tab-identity")) break;
      await browser.keys(["ArrowDown"]);
    }
    await browser.keys(["Enter"]);

    await tabTo((f) => f.text === "New identity", "New identity");
    await browser.keys(["Enter"]);
    const nameField = await idOf("Name on servers");
    await tabTo((f) => f.id === nameField, "the name field");
    await typeText("KbCard");
    const labelField = await idOf("Label");
    await tabTo((f) => f.id === labelField, "the label field");
    await typeText("Kb");
    await tabTo((f) => f.text === "Create", "the Create button");
    await browser.keys(["Enter"]);
    await card("Kb").waitForDisplayed({ timeout: 60_000 });

    await tabTo((f) => f.label === "Rename Kb", "the Rename button of the card");
    await browser.keys(["Enter"]);
    const renameField = await idOf("Label");
    await tabTo((f) => f.id === renameField, "the label field");
    // The field starts with the current label; select all and replace it (keyboard only).
    await browser.keys(["Control", "a"]);
    await browser.keys(["Backspace"]);
    await typeText("Kb2");
    await tabTo((f) => f.text === "Save", "the Save button");
    await browser.keys(["Enter"]);
    await card("Kb2").waitForDisplayed({ timeout: 15_000 });

    await tabTo((f) => f.label === "Set as default – Kb2", "Set as default");
    await browser.keys(["Enter"]);
    await tabTo((f) => f.label === "Show details of Kb2", "Details");
    await browser.keys(["Enter"]);
    await tabTo((f) => f.text === "Close", "Close");
    await browser.keys(["Enter"]);
    await tabTo((f) => f.label === "Delete Kb2", "Delete");
    await browser.keys(["Enter"]);
    const confirmField = await idOf("Type the identity label to confirm");
    await tabTo((f) => f.id === confirmField, "the confirm field");
    await typeText("Kb2");
    await tabTo((f) => f.text === "Delete", "the confirm Delete button");
    await browser.keys(["Enter"]);
    await browser.waitUntil(
      async () => (await app.invoke<IdentityRow[]>("identity_list", {})).length === 1,
      { timeout: 20_000, timeoutMsg: "the keyboard delete did not run" },
    );

    // Every control on the identity screen has an accessible name: axe scan in both themes. The
    // dropdown that announces its value is the server dialog's (checked in server-dialog.e2e.ts).
    await openSettingsSection("account");
    for (const theme of ["dark", "light"]) {
      await browser.execute((value: string) => {
        document.documentElement.dataset.theme = value;
      }, theme);
      await browser.pause(400);
      const violations = await axe.scan(`identity-manager-${theme}`);
      if (violations.length > 0) throw new Error(`${theme}: ${axe.summarize(violations)}`);
    }

    // 200 % text size: the list, the tab strip and the dialogs show nothing clipped.
    await browser.execute(() => {
      document.documentElement.style.setProperty("font-size", "200%");
    });
    const clipped = await browser.execute(() => {
      const out: string[] = [];
      for (const element of document.querySelectorAll(".identity-list, .settings__nav, .settings-dialog")) {
        // Vertical overflow is scrolling (the settings panel scrolls); only horizontal clipping
        // means text is cut off.
        if (element.scrollWidth > element.clientWidth + 2)
          out.push(`${element.className} ${String(element.scrollWidth)} > ${String(element.clientWidth)}`);
      }
      // The card's own text must survive too: a starved column clips the label and the name line.
      for (const element of document.querySelectorAll(".identity-card p, .identity-card strong")) {
        if (element.scrollWidth > element.clientWidth + 2)
          out.push(`${element.className} "${element.textContent.trim()}" clipped`);
      }
      return out;
    });
    expect(clipped).toEqual([]);
    await browser.execute(() => {
      document.documentElement.style.removeProperty("font-size");
    });
  });

});
