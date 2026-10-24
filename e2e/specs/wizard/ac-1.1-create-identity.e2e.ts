import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { app } from "../../helpers/index.ts";
import {
  button,
  keyringDump,
  labelled,
  mainWindowMarker,
  waitForText,
} from "../../helpers/wizard-util.ts";

// AC-1.1: First start → wizard → create identity "Tester", keep defaults, finish → main window
// within 60 s of starting; the certificate is in the OS secure storage and not a plain file.
// FR-1.48d: ticking "Start Mumcord when I log in" on the last step writes the XDG autostart entry.
function walk(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    return e.isDirectory() ? walk(p) : e.isFile() ? [p] : [];
  });
}

describe("AC-1.1 wizard creates identity", () => {
  it("first start → wizard → Tester → main window within 60 s", async () => {
    // Measure from a cold start of a fresh install (driver + app launch included).
    const t0 = Date.now();
    await app.restart({ freshProfile: true });
    await waitForText("Your identity", 30_000);
    await waitForText("Step 1 of 3");
    await (await labelled("Name")).setValue("Tester");
    await button("Next").click();
    await waitForText("Step 2 of 3");
    await button("Next").click();
    await waitForText("Step 3 of 3");
    await $('//label[normalize-space(.)="Start Mumcord when I log in"]//input').click();
    await button("Finish").click();
    await mainWindowMarker().waitForExist({ timeout: 30_000 });
    await expect(mainWindowMarker()).toBeDisplayed();
    const elapsed = (Date.now() - t0) / 1000;
    console.log(`AC-1.1: main window after ${elapsed.toFixed(1)} s`);
    expect(elapsed).toBeLessThan(60);
  });

  it("writes the autostart entry into the profile's config dir (FR-1.48d)", async () => {
    const entry = join(app.profile(), "config", "autostart", "app.mumcord.client.desktop");
    await browser.waitUntil(() => existsSync(entry), {
      timeout: 10_000,
      timeoutMsg: `autostart entry ${entry} never appeared`,
    });
    const lines = readFileSync(entry, "utf8").split("\n");
    expect(lines).toContain("Type=Application");
    expect(lines.some((l) => l.startsWith("Exec=") && l.endsWith(" --autostart"))).toBe(true);
  });

  it("stores the certificate in the keyring, not as a plain file", async () => {
    // Secrets are binary (PKCS#12, chunked as identity/0, identity/1, …); log only the names.
    const accounts = [...(await keyringDump()).matchAll(/attribute\.username = (\S+)/g)].map(
      (m) => m[1],
    );
    console.log(`AC-1.1 keyring accounts: ${accounts.join(", ")}`);
    expect(accounts).toContain("identity/0");

    const secret = execFileSync("secret-tool", [
      "lookup",
      "service",
      "app.mumcord.client",
      "username",
      "identity/0",
    ]);
    expect(secret.length).toBeGreaterThan(100);
    // PKCS#12 is DER: starts with a SEQUENCE tag.
    expect(secret[0]).toBe(0x30);

    const files = walk(app.profile());
    console.log(`AC-1.1 profile files:\n${files.map((f) => `${f} (${String(statSync(f).size)} B)`).join("\n")}`);
    const suspicious = files.filter((f) => /\.(p12|pfx|pem|der|key|crt|cer)$/i.test(f));
    expect(suspicious).toEqual([]);
    // A 48-byte slice of the stored secret must not appear in any profile file either.
    const probe = secret.subarray(20, 68);
    for (const f of files) {
      const content = readFileSync(f);
      expect(content.includes("PRIVATE KEY")).toBe(false);
      expect(content.includes("BEGIN CERTIFICATE")).toBe(false);
      expect(content.includes(probe)).toBe(false);
    }
  });

  it("keeps the identity after a restart (no wizard again)", async () => {
    await app.restart();
    await mainWindowMarker().waitForExist({ timeout: 30_000 });
  });
});
