// #154: right-clicking the voice bar's hang-up button opens a menu. "Leave voice" stops voice but
// keeps the connection (self-deafened, read-only notice, chat stays live); joining a channel gives
// voice back. Escape closes the menu and returns the focus to the button.
import { $, browser, expect } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import { channelName, finishWizard } from "../../helpers/chat-ui.ts";
import { showAllPeople } from "../../helpers/voice-ui.ts";

const ME = "LeaveVoiceQA";
const WATCH = { name: "LvWatcher", timeout: 30, port: testenv.ports.v15 };

const hangUp = () => $("button[aria-label^='Disconnect from ']");
const notice = () => $(".userpanel-readonly");
const menu = () => $("[role='menu'][aria-label='Disconnect options']");
const item = (name: string) => $(`//*[@role='menuitem'][.//span[normalize-space(.)='${name}']]`);

/** Right-clicks the hang-up button with a script event: WebKitWebDriver releases a right-click as
 * a left button, which hangs up now that the menu opens above the button instead of under the
 * pointer (see openServerMenu in servers-ui.ts). */
async function rightClickHangUp(): Promise<void> {
  await browser.execute(() => {
    const button = document.querySelector("button[aria-label^='Disconnect from ']");
    const box = button?.getBoundingClientRect();
    if (!button || !box) throw new Error("hang-up button not found");
    button.dispatchEvent(
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        button: 2,
        clientX: box.left + box.width / 2,
        clientY: box.top + box.height / 2,
      }),
    );
  });
}

describe("disconnect button menu (#154)", () => {
  before(async () => {
    await testenv.up("v15");
    await app.restart();
  });

  it("opens on right-click and closes with Escape, returning the focus", async () => {
    await finishWizard(ME);
    const input = $("//label[.='Server address or mumble:// link']/following::input[1]");
    await input.waitForDisplayed({ timeout: 30_000 });
    await input.setValue(testenv.address("v15"));
    await $("//button[normalize-space(.)='Add and connect']").click();
    await $("//button[.='Trust and connect']").waitForDisplayed({ timeout: 20_000 });
    await $("//button[.='Trust and connect']").click();
    await channelName("Raid").waitForDisplayed({ timeout: 30_000 });
    await showAllPeople();
    await notice().waitForDisplayed({ timeout: 15_000 });
    await $(".userpanel-readonly__join").click();
    await notice().waitForExist({ reverse: true, timeout: 15_000 });

    await rightClickHangUp();
    await menu().waitForDisplayed({ timeout: 5_000 });
    await browser.keys("Escape");
    await menu().waitForExist({ reverse: true, timeout: 5_000 });
    expect(await hangUp().isFocused()).toBe(true);
  });

  it("Leave voice keeps the connection read-only; joining gives voice back", async () => {
    await rightClickHangUp();
    await item("Leave voice").waitForDisplayed({ timeout: 5_000 });
    await item("Leave voice").click();
    await notice().waitForDisplayed({ timeout: 15_000 });
    // WebKit's getText skips ellipsized spans, so read the DOM text.
    expect(await notice().getProperty("textContent")).toContain("Read-only");
    // Still connected: the hang-up button stays and the server shows the user deafened.
    await hangUp().waitForDisplayed();
    await bot(["expect", "user", ME, "--deaf", "true"], WATCH);

    await $(".userpanel-readonly__join").click();
    await notice().waitForExist({ reverse: true, timeout: 15_000 });
    await bot(["expect", "user", ME, "--deaf", "false"], WATCH);
  });
});
