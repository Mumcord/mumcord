import { browser, expect, $ } from "@wdio/globals";
import { app, testenv } from "../../helpers/index.ts";
import { addFirstServer, channelName, finishWizard, joinChannel } from "../../helpers/chat-ui.ts";

const treeItem = (name: string) =>
  $(
    `//span[contains(@class,'tree-channel__name') and normalize-space(.)='${name}']/ancestor::div[@role='treeitem'][1]`,
  );
const menu = () => $("//div[@role='menu']");
const menuItems = async () =>
  browser.execute(() =>
    Array.from(document.querySelectorAll('[role="menu"] [role="menuitem"]')).map((item) => {
      const hint = item.querySelector(".server-menu__end")?.textContent ?? "";
      const text = item.textContent;
      return hint === "" ? text : text.slice(0, text.length - hint.length);
    }),
  );

/** A script contextmenu event, not a WebDriver right-click (see theme-language.e2e.ts). */
async function rightClick(name: string): Promise<void> {
  await browser.execute((channel) => {
    const label = Array.from(document.querySelectorAll(".tree-channel__name")).find(
      (span) => span.textContent.trim() === channel,
    );
    const row = label?.closest('[role="treeitem"]');
    if (!row) throw new Error(`no tree row for ${channel}`);
    const box = row.getBoundingClientRect();
    row.dispatchEvent(
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        button: 2,
        clientX: box.left + 8,
        clientY: box.top + 8,
      }),
    );
  }, name);
  await menu().waitForDisplayed();
}

// AC-3.65 (issue #172): right-click on a channel opens the shared channel menu; items the user
// may not use (Move users here… without the move permission) are left out; the menu key opens
// it from the keyboard and "Join channel" joins.
describe("chat: channel tree right-click menu (AC-3.65)", () => {
  before(async () => {
    await testenv.up("v15");
    await app.restart();
    await finishWizard();
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
  });

  it("offers the channel actions and hides Move users here… without the move permission", async () => {
    await rightClick("Raid");
    const items = await menuItems();
    expect(items).toEqual([
      "Join channel",
      "Follow channel",
      "Audio for this channel…",
      "Whisper to…",
      "Copy channel link",
      "Channel info",
      "Clear chat history…",
    ]);
    await browser.keys("Escape");
    await expect(menu()).not.toExist();
  });

  it("leaves out Join channel on the own voice channel", async () => {
    await rightClick("Lobby");
    expect(await menuItems()).not.toContain("Join channel");
    await browser.keys("Escape");
  });

  it("opens from the keyboard and Escape returns to the row", async () => {
    await channelName("Raid").click();
    await expect(treeItem("Raid")).toBeFocused();
    await browser.keys(["Shift", "F10"]);
    await menu().waitForDisplayed();
    await browser.keys("Escape");
    await expect(menu()).not.toExist();
    await expect(treeItem("Raid")).toBeFocused();
  });

  it("opens channel info and joins from the menu", async () => {
    await rightClick("Raid");
    await $("//button[@role='menuitem'][contains(., 'Channel info')]").click();
    const info = $("//dialog[contains(@class,'channel-info')]");
    await info.waitForDisplayed();
    await browser.keys("Escape");
    await expect(info).not.toBeDisplayed();

    await rightClick("Raid");
    await $("//button[@role='menuitem'][contains(., 'Join channel')]").click();
    await browser.waitUntil(
      // "you are here" may be visually hidden in a narrow tree; the text is still there.
      async () =>
        String(await treeItem("Raid").getProperty("textContent")).includes("you are here"),
      { timeoutMsg: "did not join Raid from the menu" },
    );
  });
});
