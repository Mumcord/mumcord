import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { browser, expect, $ } from "@wdio/globals";
import { bot, botInBackground, testenv } from "../../helpers/index.ts";

// AC-1.19 (register on this server) and AC-1.20 (own/bot avatar + comment), spec 01 §11.
// A name no other spec uses: this spec registers it on v15, and a registered name rejects every
// other identity ("wrong certificate or password"), which broke later specs connecting as "Tester".
const ME = "ProfileQA";
const PNG_1X1 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

async function clickText(selector: string, text: string): Promise<void> {
  const el = $(`${selector}=${text}`);
  await el.waitForClickable();
  await el.click();
}

async function clickItem(text: string): Promise<void> {
  const el = $(`//*[@role="menuitem"][normalize-space()="${text}"]`);
  await el.waitForClickable();
  await el.click();
}

async function onStep(n: number): Promise<void> {
  await browser.waitUntil(async () => (await $("#root").getText()).includes(`Step ${String(n)} of`), {
    timeoutMsg: `wizard step ${String(n)} not shown`,
  });
}

async function openUserMenu(): Promise<void> {
  const btn = $("button.userpanel__user");
  await btn.waitForEnabled();
  await btn.click();
  await $('[role="menu"]').waitForDisplayed();
}

describe("profile: register, avatar, comment", () => {
  let avatarPath = "";
  before(async () => {
    await testenv.reset("v15");
    avatarPath = join(mkdtempSync(join(tmpdir(), "mumcord-avatar-")), "me.png");
    writeFileSync(avatarPath, Buffer.from(PNG_1X1, "base64"));

    // Wizard: identity -> mic -> talk mode.
    await $("#root").waitForDisplayed();
    const name = $('input[type="text"]');
    await name.waitForDisplayed();
    await name.setValue(ME);
    await clickText("button", "Next");
    await onStep(2);
    await clickText("button", "Next");
    await onStep(3);
    await clickText("button", "Finish");

    // First server, then trust its certificate.
    const address = $('input[placeholder="mumble.example.org"]');
    await address.waitForDisplayed();
    await address.setValue(`localhost:${String(testenv.ports.v15)}`);
    await clickText("button", "Add and connect");
    await clickText("button", "Trust and connect");
    await $("button.userpanel__user").waitForEnabled({ timeout: 30_000 });
  });

  it("AC-1.19 offers registration to an unregistered user", async () => {
    await bot(["expect", "user", ME, "--registered", "false"], { name: "Checker" });
    await openUserMenu();
    await expect($('//*[@role="menuitem"][normalize-space()="Register on this server"]')).toBeDisplayed();
  });

  it("AC-1.19 shows the dialog with the backup hint", async () => {
    await clickItem("Register on this server");
    const dlg = $("dialog[open], [role=dialog]");
    await dlg.waitForDisplayed();
    const text = await dlg.getText();
    expect(text).toContain("Save a backup copy now");
    await expect($("button=Export identity")).toBeDisplayed();
  });

  it("AC-1.19 confirming registers the user and removes the option", async () => {
    await clickText("button", "Register");
    await browser.waitUntil(async () => !(await $("[role=dialog], dialog[open]").isDisplayed()), {
      timeoutMsg: "register dialog did not close",
    });
    await bot(["expect", "user", ME, "--registered", "true"], { name: "Checker" });
    await openUserMenu();
    await expect($('//*[@role="menuitem"][normalize-space()="Register on this server"]')).not.toBeExisting();
    await browser.keys("Escape");
  });

  it("AC-1.20 own avatar and comment reach the bot", async () => {
    // The native file picker cannot be driven by WebDriver: call the same engine command the
    // dialog calls after the picker returns.
    await browser.execute(async (p: string) => {
      const w = window as unknown as {
        __TAURI_INTERNALS__: { invoke: (c: string, a: object) => Promise<unknown> };
      };
      await w.__TAURI_INTERNALS__.invoke("set_own_avatar", { path: p });
    }, avatarPath);
    await openUserMenu();
    await clickItem("Edit profile");
    const area = $("textarea");
    await browser.waitUntil(async () => await area.isEnabled());
    await area.setValue(`Hello from ${ME}`);
    await clickText("button", "Save");
    await bot(
      ["expect", "user", ME, "--has-avatar", "true", "--comment-contains", `Hello from ${ME}`],
      { name: "Checker" },
    );
  });

  it("AC-1.20 the bot's avatar and comment show in member list and profile card", async () => {
    const b = botInBackground(["script"], { name: "Bot", hold: 90 });
    try {
      b.send("avatar --sample");
      b.send('comment "Bot comment XYZ"');
      await bot(
        ["expect", "user", "Bot", "--has-avatar", "true", "--comment-contains", "XYZ"],
        { name: "Checker" },
      );
      // Exact name (the name's own text, not its hidden status): other people on the server, e.g.
      // scale bots, may have "Bot" in theirs, and the list may need scrolling to reach the row.
      const row = $(
        "//button[contains(@class,'member')][.//span[contains(@class,'member__name') and normalize-space(text()[1])='Bot']]",
      );
      await row.waitForExist();
      await row.scrollIntoView({ block: "center" });
      await row.waitForDisplayed();
      // Member list: a picture (or comment) for the bot.
      const listImg = row.$("img");
      const listHasImg = await listImg.isExisting();
      // A click pins the preview card (FR-1.62); its "Full profile" button opens the full card.
      await row.click();
      const full = $("//div[@id='profile-preview']//button[normalize-space(.)='Full profile']");
      await full.waitForDisplayed();
      await full.click();
      const card = $("dialog.profile-card");
      await card.waitForDisplayed();
      await browser.waitUntil(async () => (await card.getText()).includes("Bot comment XYZ"), {
        timeoutMsg: "comment not in profile card",
      });
      await expect(card.$("img")).toBeExisting();
      expect(listHasImg).toBe(true);
    } finally {
      b.stop();
      await b.done.catch(() => undefined);
    }
  });
});
