import { browser, expect, $ } from "@wdio/globals";
import { app, testenv } from "../../helpers/index.ts";
import { addFirstServer, finishWizard, joinChannel, selectChannel } from "../../helpers/chat-ui.ts";

const channelName = () =>
  $("//h2[contains(@class,'chat-header__title')]//button[@aria-haspopup='menu']");
const menuItem = (text: string) => $(`//button[@role='menuitem'][contains(., '${text}')]`);
const audioDialog = () => $("//dialog[contains(@class,'channel-audio')]");
const quickPanel = () => $("//div[@role='dialog' and @aria-label='Voice quality']");
const quickSwitch = (label: string) =>
  $(`//div[@role='dialog' and @aria-label='Voice quality']//label[.='${label}']`);

async function openQuickPanel(): Promise<void> {
  // The arrow next to the connection chip opens the panel (right-click and the menu key also do).
  await $("//button[@aria-label='Voice quality']").click();
  await quickPanel().waitForDisplayed();
}

async function closeQuickPanel(): Promise<void> {
  await browser.keys("Escape");
  await quickPanel().waitForDisplayed({ reverse: true });
}

/** Checked state of the switch with `label` in the quick panel. */
async function switchOn(label: string): Promise<boolean> {
  const id = await quickSwitch(label).getAttribute("for");
  if (id === null) throw new Error(`no switch "${label}"`);
  return $(`[id='${id}']`).isSelected();
}

// Issue #171 (FR-3.62–FR-3.64): a channel audio profile applies when the user joins that channel
// and the global settings come back when they leave it. The quick panel names the source.
describe("voice: per-channel audio profiles (#171)", () => {
  before(async () => {
    await testenv.up("v15");
    await app.restart();
    await finishWizard();
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
    await selectChannel("Raid");
  });

  it("saves a Music profile for a channel from the channel-name menu", async () => {
    await channelName().click();
    await menuItem("Audio for this channel").click();
    await audioDialog().waitForDisplayed();
    await $(
      "//dialog[contains(@class,'channel-audio')]//label[contains(., 'Music')][.//input[@type='radio']]",
    ).click();
    await $("//dialog[contains(@class,'channel-audio')]//button[.='Save']").click();
    await audioDialog().waitForDisplayed({ reverse: true });
    await expect($("//span[contains(@class,'channel-header__badge')]")).toHaveText(
      expect.stringContaining("Music profile"),
    );
  });

  it("outside the channel the global settings stay in effect", async () => {
    await openQuickPanel();
    await expect(quickSwitch("Music quality")).toBeDisplayed();
    expect(await switchOn("Music quality")).toBe(false);
    await closeQuickPanel();
  });

  it("joining the channel applies the profile and names its source", async () => {
    await joinChannel("Raid");
    await openQuickPanel();
    await quickSwitch("Music quality (channel profile)").waitForDisplayed();
    expect(await switchOn("Music quality (channel profile)")).toBe(true);
    await expect(
      $(
        "//div[@role='dialog' and @aria-label='Voice quality']//label[.='Bitrate (channel profile)']",
      ),
    ).toBeDisplayed();
    await closeQuickPanel();
    await $("//button[@aria-label='Connection info']").click();
    await expect($("//dt[.='Audio profile']/following-sibling::dd[1]")).toHaveText(
      "Music quality, 256 kbit/s (channel profile)",
    );
    await browser.keys("Escape");
  });

  it("leaving the channel restores the global settings", async () => {
    await joinChannel("Lobby");
    await openQuickPanel();
    await quickSwitch("Music quality").waitForDisplayed();
    expect(await switchOn("Music quality")).toBe(false);
    await expect(quickSwitch("Music quality (channel profile)")).not.toExist();
    await closeQuickPanel();
  });
});
