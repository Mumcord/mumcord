// Spec 07 file sharing, the ways into the composer in the running client against the mumpeer test
// environment (helpers/relayenv.ts). A = Alice offers, B = Bob downloads.
//
//   F6     the paperclip opens the system file picker; the picked file becomes a chip.
//   F6/#11 dropping an image into a channel asks "Send as image" or "Share as file" (screenshots
//          in dark and light: e2e/artifacts/files-drop-choice-{dark,light}.png); either answer works.
//   AC-7.21 a dropped folder is zipped (chip progress) and offered as "{folder}.zip"; Bob's copy
//          holds the original files; A's cached zip is gone after Stop sharing.
//   AC-7.24 Copy puts the full 64-hex checksum on the clipboard; the card can be driven from the
//           keyboard from Download to Done.
//
// Skips itself without a mumpeer checkout.
import { execFileSync } from "node:child_process";
import { mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { browser, $, expect } from "@wdio/globals";
import { app, apps, relayenv } from "../../helpers/index.ts";
import { readClipboard } from "../../helpers/chat-ui.ts";
import * as files from "../../helpers/files-ui.ts";
import { artifactsDir } from "../../helpers/paths.ts";
import { addAndConnect, completeWizard, personRow } from "../../helpers/voice-ui.ts";

async function joinVoice(): Promise<void> {
  const join = $(".userpanel-readonly__join");
  if (!(await join.isDisplayed())) return;
  await join.click();
  await $(".userpanel-readonly").waitForExist({ reverse: true, timeout: 15_000 });
}

/** A small 16×16 PNG. */
function png(dir: string, name: string): string {
  const path = join(dir, name);
  writeFileSync(
    path,
    Buffer.from(
      "iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAIAAACQkWg2AAABlElEQVR4nAXBwQAAIAxA0SEMIYQQQgghhI7/OIQQhjCEEIYwhBB6T0RQoQldGMIUlrAFE47gQghXSKGEJ4goqjSlK0OZylK2YspRXAnlKqmU8hSRhjZaozdGYzZWYzescRreiMZtZKMaryHS0U7r9M7ozM7q7I51Tsc70bmd7FTndUQGOmiDPhiDOViDPbDBGfggBneQgxq8gchEJ23SJ2MyJ2uyJzY5E5/E5E5yUpM3EVnooi36YizmYi32whZn4YtY3EUuavEWIhvdtE3fjM3crM3e2OZsfBObu8lNbd5GxFCjGd0YxjSWsQ0zjuFGGNdIo4xniBz00A79MA7zsA77YIdz8EMc7iEPdXgHEUed5nRnONNZznbMOY474VwnnXKeIxJo0IIejGAGK9iBBSfwIIIbZFDBC0QuemmXfhmXeVmXfbHLufglLveSl7q8i0iiSUt6MpKZrGQnlpzEk0hukkklLxEptGhFL0Yxi1XswopTeBHFLbKo4hUiD320R3+Mx3ysx37Y4zz8EY/7yEc93uMDT05oED0ha2QAAAAASUVORK5CYII=",
      "base64",
    ),
  );
  return path;
}

/** Quick settings → All settings → Appearance → "Dark or light". */
async function themeMode(label: "Dark" | "Light"): Promise<void> {
  await $('button[aria-label="Quick settings"]').click();
  await $(".quick-settings").waitForDisplayed();
  await $("//*[contains(@class,'quick-settings')]//button[contains(normalize-space(.),'All settings')]").click();
  await $(".overlay__dialog").waitForDisplayed();
  await $('[role="tab"]=Appearance').click();
  await $("//label[normalize-space(.)='Dark or light']/following::*[@role='combobox'][1]").click();
  await $(`//*[@role='option' and normalize-space(.)='${label}']`).click();
  await browser.keys(["Escape"]);
  await $(".overlay__dialog").waitForExist({ reverse: true });
  const want = label.toLowerCase();
  await browser.waitUntil(
    async () => (await browser.execute(() => document.documentElement.dataset.theme ?? "")).includes(want),
    { timeoutMsg: `theme never became ${want}` },
  );
}

/** The image-drop question. */
const choice = () => $(".file-drop-choice");

(relayenv.relayAvailable() ? describe : describe.skip)("file sharing: composer (spec 07)", function () {
  this.timeout(900_000);
  this.bail(true);

  let port = 0;
  const dir = files.fileDir();

  after(async () => {
    await apps.stopB();
  });

  it("setup: Alice and Bob on a relay that allows files", async () => {
    port = (await relayenv.startRelay("all")).port;
    await completeWizard("Alice");
    await addAndConnect(`localhost:${String(port)}`);
    await joinVoice();
    await files.registerOnServer();
    await apps.startB();
    await apps.onB(async () => {
      await completeWizard("Bob");
      await addAndConnect(`localhost:${String(port)}`);
      await joinVoice();
      await files.registerOnServer();
    });
    await personRow("Bob").waitForDisplayed({ timeout: 30_000 });
    await files.waitSharingReady();
  });

  it("F6: the paperclip opens the system file picker; the picked file becomes a chip", async () => {
    const picked = files.randomFile(dir, "picked.txt", 4096);
    const clip = $("button[aria-label='Share files in this channel']");
    await clip.waitForClickable({ timeout: 30_000 });
    await clip.click();
    await files.pickInFileDialog("Share files in this channel", picked);
    await files.waitChipReady("picked.txt");
    await files.chip("picked.txt").$("button[aria-label='Remove picked.txt']").click();
    await files.chip("picked.txt").waitForExist({ reverse: true });
  });

  it("image drop: asks Send as image or Share as file (dark and light)", async () => {
    const image = png(dir, "sunset.png");
    mkdirSync(artifactsDir, { recursive: true });
    for (const mode of ["Dark", "Light"] as const) {
      await themeMode(mode);
      await files.dropFiles("a", [image]);
      await choice().waitForDisplayed({ timeout: 10_000, timeoutMsg: "no image-drop question" });
      expect(await choice().getText()).toContain("Some of these are images. What should happen with them?");
      const focused = await browser.execute(() => document.activeElement?.textContent.trim());
      expect(focused).toBe("Send as image");
      await browser.saveScreenshot(join(artifactsDir, `files-drop-choice-${mode.toLowerCase()}.png`));
      await browser.keys("Escape");
      await choice().waitForExist({ reverse: true });
    }
  });

  it("image drop: Share as file makes a chip, Send as image the inline picture", async () => {
    const image = join(dir, "sunset.png");
    await files.dropFiles("a", [image]);
    await choice().$("button=Share as file").click();
    await files.waitChipReady("sunset.png");
    await files.chip("sunset.png").$("button[aria-label='Remove sunset.png']").click();
    await files.dropFiles("a", [image]);
    await choice().$("button=Send as image").click();
    await $("//*[starts-with(@alt,'Image to send')]").waitForExist({ timeout: 20_000 });
    expect(await files.chip("sunset.png").isExisting()).toBe(false);
    await browser.keys("Enter");
    await apps.onB(() => $(".chat-message__body img").waitForExist({ timeout: 30_000 }));
  });

  it("AC-7.21: a dropped folder is zipped and offered as a zip", async () => {
    const folder = join(dir, "holiday");
    files.randomFile(folder, "a.bin", 3 * 1024 * 1024);
    files.randomFile(join(folder, "sub"), "b.txt", 1000);
    await files.dropFiles("a", [folder]);
    await files.waitChipReady("holiday.zip");
    await files.send();
    await files.waitCardText("holiday.zip", "shared by you");
  });

  it("AC-7.24: Copy puts the full checksum on the clipboard", async () => {
    await apps.onB(async () => {
      await files.waitCardText("holiday.zip", "Available while Alice is online");
      const copy = files.card("holiday.zip").$(".file-card__copy");
      await copy.click();
      await files.waitCardText("holiday.zip", "Copied", 5_000);
    });
    // B's clipboard lives on the shared X display.
    expect(await readClipboard("text")).toMatch(/^[0-9a-f]{64}$/);
  });

  it("AC-7.21/7.24: Bob downloads the zip with the keyboard only; it holds the folder", async () => {
    await apps.onB(async () => {
      const button = files.cardButton("holiday.zip", "Download");
      await browser.execute((el: HTMLElement) => {
        el.focus();
      }, await button);
      await browser.keys("Enter");
      await files.dialog().waitForDisplayed();
      const focused = await browser.execute(() => document.activeElement?.textContent.trim());
      expect(focused).toBe("Download");
      await browser.keys("Enter");
      await files.waitDownloaded("holiday.zip");
    });
    const zip = join(app.downloadsDir("b"), "Mumcord", "holiday.zip");
    const listing = execFileSync("python3", ["-I", "-m", "zipfile", "-l", zip], { encoding: "utf8" });
    expect(listing).toContain("a.bin");
    expect(listing).toContain("sub/b.txt");
  });

  it("AC-7.21: Stop sharing deletes A's cached zip", async () => {
    const cached = (): string[] => {
      const out = execFileSync("find", [app.profile(), "-path", "*file-shares/*", "-type", "f"], {
        encoding: "utf8",
      });
      return out.split("\n").filter((line) => line !== "");
    };
    expect(cached().length).toBeGreaterThan(0);
    await files.cardButton("holiday.zip", "Stop sharing").click();
    await files.waitCardText("holiday.zip", "You stopped sharing");
    await browser.waitUntil(() => cached().length === 0, {
      timeout: 10_000,
      timeoutMsg: `cached zip still there: ${cached().join(", ")}`,
    });
    expect(readdirSync(dir)).toContain("holiday");
  });
});
