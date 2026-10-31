import { browser, expect, $ } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  finishWizard,
  joinChannel,
  selectChannel,
  USER,
} from "../../helpers/chat-ui.ts";

const followEye = () => $("//button[contains(@class,'follow-toggle') and @aria-pressed]");
const channelName = () =>
  $("//h2[contains(@class,'chat-header__title')]//button[@aria-haspopup='menu']");
const menuItem = (text: string) => $(`//button[@role='menuitem'][contains(., '${text}')]`);
const infoDialog = () => $("//dialog[contains(@class,'channel-info')]");
const infoRow = (label: string) =>
  $(`//dialog[contains(@class,'channel-info')]//dt[.='${label}']/following-sibling::dd[1]`);

// AC-3.66 (issue #170): the follow eye toggles following; the channel-name menu opens channel
// info with the sanitised description, limits and the own permissions.
describe("chat: channel header menu and channel info (AC-3.66)", () => {
  before(async () => {
    await testenv.up("v15");
    await bot(["describe", "Raid", "<b>Raid</b> nights<script>window.pwned=1</script>"], {
      superuser: true,
    });
    await app.restart();
    await finishWizard();
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
    await selectChannel("Raid");
  });

  after(async () => {
    await bot(["describe", "Raid", ""], { superuser: true }).catch(() => undefined);
  });

  it("the follow eye in front of the name toggles following", async () => {
    await expect(followEye()).toHaveAttribute("aria-pressed", "false");
    await followEye().click();
    await expect(followEye()).toHaveAttribute("aria-pressed", "true");
    await bot(["expect", "user", USER, "--listening", "Raid"], { name: "EyeBot" });
    // The menu shows the same state and unfollows too.
    await channelName().click();
    await menuItem("Unfollow channel").click();
    await expect(followEye()).toHaveAttribute("aria-pressed", "false");
    await bot(["expect", "user", USER, "--not-listening", "Raid"], { name: "EyeBot" });
  });

  it("the menu opens from the keyboard and Escape returns to the name", async () => {
    await channelName().click();
    await expect($("//div[@role='menu']")).toBeDisplayed();
    await browser.keys("Escape");
    await expect($("//div[@role='menu']")).not.toExist();
    await expect(channelName()).toBeFocused();
  });

  it("Channel info shows the sanitised description, limits and own permissions", async () => {
    await channelName().click();
    await menuItem("Channel info").click();
    await infoDialog().waitForDisplayed();
    const description = $("//dialog[contains(@class,'channel-info')]//div[contains(@class,'channel-info__description')]");
    await browser.waitUntil(async () => (await description.getText()).includes("Raid nights"), {
      timeoutMsg: "description not shown",
    });
    expect(await description.$$("script").length).toBe(0);
    expect(await browser.execute(() => "pwned" in window)).toBe(false);
    await expect(infoRow("Users")).toHaveText(/\d/);
    await expect(infoRow("Type")).toHaveText("Permanent");
    await expect(infoRow("My permissions")).toHaveText(/^Enter, Speak, Text/);
    await expect(infoRow("Quality limit")).toHaveText(/kbit\/s/);
    await browser.keys("Escape");
    await expect(infoDialog()).not.toBeDisplayed();
  });
});
