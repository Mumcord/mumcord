// In-app updater (#168): a local stand-in for GitHub serves the release list, the update manifest,
// a signed fake AppImage and SHA256SUMS. The app (a dev build pretending to be its Cargo version)
// finds 9.9.9, rejects a tampered download without touching the installed file, and installs the
// genuine one in place. "Restart to update" is never clicked: the app must not restart in a test.
import { createHash, randomBytes } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { browser, expect, $ } from "@wdio/globals";
import { app, repoRoot } from "../../helpers/index.ts";
import { runOk } from "../../helpers/run.ts";
import { button, completeWizard } from "../../helpers/servers-ui.ts";
import { waitFor, waitGone, waitText } from "./flows.ts";

const VERSION = "9.9.9";
const FILE = `Mumcord_${VERSION}_amd64.AppImage`;
const NEWEST_NOTE = "Updater fixture change in 9.9.9";
const OLDER_NOTE = "Updater fixture fix in 9.9.8";
const CHUNK = 64 * 1024;
const CHUNK_DELAY_MS = 80;

const tmp = mkdtempSync(join(tmpdir(), "mumcord-updater-"));
const appImage = join(tmp, "Mumcord.AppImage");
const installed = Buffer.from("\x7fELF installed fake AppImage", "latin1");
// A 2 MB "AppImage": big enough that the download takes a few seconds in chunks.
const bundle = Buffer.concat([Buffer.from("\x7fELF", "latin1"), randomBytes(2 * 1024 * 1024)]);
let tampered = false;
let server: Server | undefined;

function release(version: string, date: string, body: string) {
  return { tag_name: `v${version}`, prerelease: false, draft: false, published_at: date, body };
}

/** GitHub's release list, the manifest, the bundle (in slow chunks) and SHA256SUMS. */
function startServer(signature: string): Promise<string> {
  const releases = JSON.stringify([
    release(
      VERSION,
      "2026-10-30T12:00:00Z",
      `## What's new in ${VERSION}\n\n### Added\n\n- ${NEWEST_NOTE}\n\n## Downloads\n\n- Not part of the notes`,
    ),
    release("9.9.8", "2026-10-20T12:00:00Z", `## What's new in 9.9.8\n\n### Fixed\n\n- ${OLDER_NOTE}\n`),
    release("0.0.1", "2020-01-01T12:00:00Z", "## What's new in 0.0.1\n\n### Added\n\n- Too old to show\n"),
  ]);
  const sums = `${createHash("sha256").update(bundle).digest("hex")}  ${FILE}\n`;
  const srv = createServer((req, res) => {
    const base = `http://127.0.0.1:${String((srv.address() as AddressInfo).port)}`;
    const folder = `/download/v${VERSION}/`;
    switch (req.url) {
      case "/releases":
        res.writeHead(200, { "content-type": "application/json" }).end(releases);
        return;
      case `${folder}latest.json`:
        res.writeHead(200, { "content-type": "application/json" }).end(
          JSON.stringify({
            version: VERSION,
            notes: NEWEST_NOTE,
            pub_date: "2026-10-30T12:00:00Z",
            platforms: { "linux-x86_64-appimage": { url: `${base}${folder}${FILE}`, signature } },
          }),
        );
        return;
      case `${folder}SHA256SUMS`:
        res.writeHead(200, { "content-type": "text/plain" }).end(sums);
        return;
      case `${folder}${FILE}`: {
        const body = Buffer.from(bundle);
        // Changed after signing: the signature no longer matches the bytes.
        if (tampered) body[body.length - 1] ^= 0xff;
        res.writeHead(200, {
          "content-type": "application/octet-stream",
          "content-length": String(body.length),
        });
        let offset = 0;
        const next = () => {
          if (res.destroyed) return;
          if (offset >= body.length) {
            res.end();
            return;
          }
          res.write(body.subarray(offset, offset + CHUNK));
          offset += CHUNK;
          setTimeout(next, CHUNK_DELAY_MS);
        };
        next();
        return;
      }
      default:
        res.writeHead(404).end();
    }
  });
  server = srv;
  return new Promise((resolvePromise, reject) => {
    srv.once("error", reject);
    srv.listen(0, "127.0.0.1", () => {
      resolvePromise(`http://127.0.0.1:${String((srv.address() as AddressInfo).port)}`);
    });
  });
}

/** Signs the bundle with a throwaway key made for this run; returns [pubkey, signature]. */
async function signBundle(): Promise<[string, string]> {
  const tauri = join(repoRoot, "node_modules", ".bin", "tauri");
  const key = join(tmp, "k.key");
  const file = join(tmp, FILE);
  writeFileSync(file, bundle);
  await runOk(tauri, ["signer", "generate", "--ci", "-p", "", "-w", key]);
  await runOk(tauri, ["signer", "sign", "-f", key, "-p", "", "--app-version", VERSION, file]);
  return [readFileSync(`${key}.pub`, "utf8").trim(), readFileSync(`${file}.sig`, "utf8").trim()];
}

const updateDialog = () => $("dialog[open][aria-labelledby]");
const checkNow = () => button("Check now");

async function openUpdatesSettings(): Promise<void> {
  if (!(await $(".overlay__dialog").isExisting())) {
    await $('button[aria-label="Quick settings"]').click();
    await waitFor(".quick-settings");
    await $("//*[contains(@class,'quick-settings')]//button[contains(normalize-space(.),'All settings')]").click();
    await waitFor(".overlay__dialog");
  }
  await $('[role="tab"]=General').click();
  await expect(checkNow()).toBeDisplayed();
}

/** Picks "When an update is found" (the radios are visually hidden inside their labels). */
async function chooseMode(label: string, value: string): Promise<void> {
  await $(`label=${label}`).click();
  await expect($(`[role="radiogroup"] input[value="${value}"]`)).toBeSelected();
}

describe("in-app updater (#168)", function () {
  this.bail(true);

  before(async () => {
    writeFileSync(appImage, installed);
    const [pubkey, signature] = await signBundle();
    const url = await startServer(signature);
    await app.restart({
      env: { MUMCORD_UPDATE_URL: url, MUMCORD_TEST_UPDATER_PUBKEY: pubkey, APPIMAGE: appImage },
    });
  });

  // The next spec file starts a fresh app without the extra env (app.prepareFresh).
  after(() => {
    server?.close();
    rmSync(tmp, { recursive: true, force: true });
  });

  it("Notify only: Check now offers the update with the notes of every newer version", async () => {
    await completeWizard("Updater Tester");
    await openUpdatesSettings();
    await chooseMode("Notify only", "notify");
    await checkNow().click();

    await expect(updateDialog()).toBeDisplayed({ wait: 20_000 });
    await waitText("dialog[open]", "Update available");
    await waitText("dialog[open]", `Mumcord ${VERSION} is ready to download`);
    await waitText("dialog[open]", NEWEST_NOTE);
    const dialog = await updateDialog().getText();
    expect(dialog).not.toContain("Not part of the notes");
    expect(dialog).not.toContain("0.0.1");
    // Older versions are listed too, collapsed until opened.
    const older = $("//dialog[@open]//button[@aria-expanded][contains(normalize-space(.),'9.9.8')]");
    await expect(older).toHaveAttribute("aria-expanded", "false");
    await older.click();
    await waitText("dialog[open]", OLDER_NOTE);

    await button("Remind me later").click();
    await waitGone("dialog[open][aria-labelledby]");

    await waitText(".overlay__dialog", `Mumcord ${VERSION} is available`);
    await expect($('[role="radiogroup"] input[value="notify"]')).toBeSelected();
    await expect(checkNow()).toBeEnabled();

    await chooseMode("Off", "off");
    await waitText(".overlay__dialog", "Update checks are off");
    await expect(checkNow()).toBeDisabled();
    await chooseMode("Notify only", "notify");
    await expect(checkNow()).toBeEnabled();
  });

  it("a tampered download fails with a signature error and leaves the AppImage alone", async () => {
    tampered = true;
    await button("Show update").click();
    await expect(updateDialog()).toBeDisplayed();
    await button("Download and install").click();

    const alert = $("dialog[open] [role='alert']");
    await expect(alert).toBeDisplayed({ wait: 30_000 });
    await expect(alert).toHaveText("The signature of the download did not match, so the download was discarded.");
    await waitText("dialog[open]", "The update did not work");
    expect(readFileSync(appImage).equals(installed)).toBe(true);
  });

  it("Retry downloads with progress, verifies and installs the update in place", async () => {
    tampered = false;
    await button("Retry").click();

    const bar = $("dialog[open] [role='progressbar']");
    await expect(bar).toBeDisplayed({ wait: 10_000 });
    await expect(bar).toHaveAttribute("aria-label", "Update progress");
    // The chunked download takes a few seconds: the bar must move before it completes.
    await browser.waitUntil(
      async () => {
        const now = Number((await bar.isExisting()) ? await bar.getAttribute("aria-valuenow") : NaN);
        return now > 0 && now < 100;
      },
      { timeout: 10_000, interval: 100, timeoutMsg: "progress bar never showed a partial download" },
    );

    await expect(button("Restart to update")).toBeDisplayed({ wait: 30_000 });
    await waitText("dialog[open]", `Mumcord ${VERSION} is ready`);
    await waitText("dialog[open]", "Signature and checksum verified");
    expect(readFileSync(appImage).equals(bundle)).toBe(true);
    // Leave without restarting: "Later" closes the dialog, the settings show the waiting update.
    await button("Later").click();
    await waitGone("dialog[open][aria-labelledby]");
    await waitText(".overlay__dialog", `Mumcord ${VERSION} is ready. Restart to update.`);
  });
});
