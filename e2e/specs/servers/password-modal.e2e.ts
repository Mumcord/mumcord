import { browser, expect, $ } from "@wdio/globals";
import { testenv } from "../../helpers/index.ts";
import { finishWizard, trustIfAsked } from "../../helpers/chat-ui.ts";
import {
  button,
  dialogTitle,
  fill,
  filesContaining,
  keyringDump,
  serverButton,
} from "../../helpers/servers-ui.ts";

// AC-2.40: the wrong-password modal (FR-2.59) against two local servers that need a password.
const RIGHT = "qa-c1-right-9d41e7";
const WRONG_SAVED = "qa-c1-saved-wrong-3b8a52";
const WRONG_TYPED = "qa-c1-typed-wrong-71c0fd";
const ONE = "PwOne";
const TWO = "PwTwo";

async function addServer(label: string, svc: "v15b" | "v14"): Promise<void> {
  await browser.$('button[aria-label="Add a server"]').click();
  await dialogTitle("Add a server").waitForDisplayed();
  await fill("Address", "127.0.0.1");
  await fill("Port", String(testenv.ports[svc]));
  await fill("Name in the server bar", label);
  await fill("Your name on this server", "Tester");
  await fill("Server password (optional)", WRONG_SAVED);
  await button("Add server").click();
  await serverButton(label).waitForDisplayed();
}

function modal(server: string) {
  return dialogTitle(`Password for ${server}`);
}

function passwordField() {
  return $("//div[@role='dialog' or @role='alertdialog']//input[@type='password' or @type='text'][1]");
}

async function barLabel(name: string): Promise<string> {
  return (await serverButton(name).getAttribute("aria-label")) ?? "";
}

async function waitConnected(name: string): Promise<void> {
  await browser.waitUntil(async () => (await barLabel(name)).includes("(connected)"), {
    timeout: 30_000,
    timeoutMsg: `${name} never connected`,
  });
}

describe("wrong server password modal (AC-2.40)", function () {
  this.bail(true);

  before(async () => {
    process.env.SERVER_PASSWORD = RIGHT;
    await testenv.reset("v15b");
    await testenv.reset("v14");
    await finishWizard("Tester");
    await $("button[aria-label='Add a server']").waitForDisplayed({ timeout: 30_000 });
    await addServer(ONE, "v15b");
    await addServer(TWO, "v14");
  });

  after(async () => {
    delete process.env.SERVER_PASSWORD;
    await testenv.up("v15b").catch(() => undefined);
    await testenv.up("v14").catch(() => undefined);
  });

  it("two servers failing at once each get their own modal; field focused and masked, Save pre-checked", async () => {
    await serverButton(ONE).click();
    await trustIfAsked();
    await serverButton(TWO).click();
    await trustIfAsked();
    await modal(ONE).waitForDisplayed({ timeout: 30_000 });
    expect(await $("//input[@type='password']").isFocused()).toBe(true);
    expect(await $("//label[contains(.,'Save password')]/input").isSelected()).toBe(true);
    // The second server's modal comes after the first is dealt with.
    await browser.keys("Escape");
    await modal(TWO).waitForDisplayed({ timeout: 15_000 });
    await browser.keys("Escape");
    await browser.waitUntil(async () => !(await modal(TWO).isExisting()), { timeout: 5_000 });
  });

  it("Cancel leaves the server disconnected with the normal error", async () => {
    expect(await barLabel(ONE)).not.toContain("(connected)");
    expect(await browser.execute(() => document.body.innerText)).toContain("server password is wrong");
  });

  it("a wrong password keeps the modal with an inline error; the right one (Enter) connects and saves", async () => {
    await serverButton(ONE).click();
    await modal(ONE).waitForDisplayed({ timeout: 30_000 });
    await passwordField().setValue(WRONG_TYPED);
    await browser.keys("Enter");
    await $("//*[@role='alert'][contains(.,'wrong')]").waitForDisplayed({ timeout: 20_000 });
    expect(await modal(ONE).isDisplayed()).toBe(true);
    await passwordField().setValue(RIGHT);
    await browser.keys("Enter");
    await waitConnected(ONE);
    expect(await modal(ONE).isExisting()).toBe(false);
    expect(await keyringDump()).toContain(RIGHT);
    expect(await keyringDump()).not.toContain(WRONG_TYPED);
  });

  it("with Save password unchecked the connect works but the saved password is unchanged", async () => {
    await serverButton(TWO).click();
    await modal(TWO).waitForDisplayed({ timeout: 30_000 });
    await $("//label[contains(.,'Save password')]/input").click();
    await passwordField().setValue(RIGHT);
    await button("Connect").click();
    await waitConnected(TWO);
    const dump = await keyringDump();
    expect(dump).toContain(WRONG_SAVED);
    // RIGHT belongs to ONE only: exactly one secret holds it.
    expect(dump.split(RIGHT).length - 1).toBe(1);
  });

  it("the passwords do not appear in the log files", () => {
    expect(filesContaining(RIGHT)).toEqual([]);
    expect(filesContaining(WRONG_TYPED)).toEqual([]);
    expect(filesContaining(WRONG_SAVED)).toEqual([]);
  });
});
