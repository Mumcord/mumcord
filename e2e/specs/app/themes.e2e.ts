// AC-2.12–AC-2.16: theme presets, Dark / Light / System versions, the Light | Dark editor with its
// contrast gate and auto-generated version, sharing (mct1. codes, theme card, "Apply to" a server),
// per-server themes, visual effects and the themed dropdowns. The second Mumcord client of AC-2.13
// is played by the app itself (receiver) and by the bot (sender / official client).
import { browser, expect, $, $$ } from "@wdio/globals";
import { app, testenv } from "../../helpers/index.ts";
import { ScriptBot, USER, composer, waitMessage } from "../../helpers/chat-ui.ts";
import { button, dialogTitle, fill, serverButton } from "../../helpers/servers-ui.ts";
import { connectFromEmptyState, waitFor, waitGone, wizard } from "./flows.ts";

const SERVER_B = "Server B";

const html = () => browser.execute(() => document.documentElement.dataset.theme ?? "");
const token = (name: string) =>
  browser.execute((n: string) => getComputedStyle(document.documentElement).getPropertyValue(n).trim(), name);
const chatBg = () => token("--color-bg-chat");

async function appearance(): Promise<void> {
  if (!(await $(".overlay__dialog").isExisting())) {
    await $('button[aria-label="Quick settings"]').click();
    await waitFor(".quick-settings");
    await $(".quick-settings button=Settings").click().catch(async () => {
      await browser.keys(["End", "Enter"]);
    });
    await waitFor(".overlay__dialog");
  }
  await $('[role="tab"]=Appearance').click();
}

async function closeSettings(): Promise<void> {
  await browser.keys(["Escape"]);
  await waitGone(".overlay__dialog");
}

/** The themed mode dropdown (Dark / Light / System) picked with the mouse. */
async function pickMode(label: string): Promise<void> {
  const combo = $("//label[normalize-space(.)='Dark or light']/following::*[@role='combobox'][1]");
  await combo.click();
  await $(`//*[@role='option' and normalize-space(.)='${label}']`).click();
}

const circle = (name: string) => $(`.theme-picker__circle[aria-label^="${name},"]`);

async function openEditorFromCurrent(): Promise<void> {
  await $(".theme-picker__circle--add").click();
  await dialogTitle("New theme").waitForDisplayed();
}

async function setHex(label: string, value: string): Promise<void> {
  const input = $(`input[aria-label="${label}"]`);
  await input.setValue(value);
  await browser.keys(["Tab"]);
}

describe("themes (AC-2.12 – AC-2.16)", function () {
  this.bail(false);
  let code = "";

  before(async () => {
    await testenv.reset("v15", { cert: "A" });
    await testenv.up("v15b", { cert: "A" });
    await app.restart({ freshProfile: true });
    await wizard(USER);
    await connectFromEmptyState(undefined, testenv.address("v15"));
  });

  after(async () => {
    await testenv.up("v15", { cert: "A" });
  });

  it("AC-2.12: every preset changes the whole app at once", async () => {
    await appearance();
    const seen = new Set<string>();
    for (const name of ["Deep Ocean", "Forest", "Paper", "Mumcord"]) {
      await circle(name).click();
      await browser.pause(300);
      seen.add(`${await chatBg()}|${await token("--color-accent")}`);
    }
    expect(seen.size).toBe(4);
    await closeSettings();
  });

  it("AC-2.12: low-contrast text warns and blocks saving", async () => {
    await appearance();
    await openEditorFromCurrent();
    const bg = await $('input[aria-label="Background, Dark"]').getValue();
    await setHex("Text, Dark", bg);
    await $(".settings-warning").waitForDisplayed();
    expect(await $("button=Save as theme").isEnabled()).toBe(false);
    await setHex("Text, Dark", "#ffffff");
    await browser.waitUntil(async () => !(await $(".settings-warning").isExisting()));
    expect(await $("button=Save as theme").isEnabled()).toBe(true);
    await button("Cancel").click();
    await closeSettings();
  });

  it("AC-2.12: Dark / Light / System flip the whole app", async () => {
    await appearance();
    await pickMode("Dark");
    await browser.waitUntil(async () => (await html()) === "dark");
    const dark = await chatBg();
    await pickMode("Light");
    await browser.waitUntil(async () => (await html()) === "light");
    const light = await chatBg();
    expect(light).not.toBe(dark);
    await pickMode("System");
    await browser.waitUntil(async () => (await html()) === "light"); // OS is light in the harness
    await closeSettings();
  });

  it("AC-2.12: a custom theme made in dark gets a passing light version, both editable", async () => {
    await appearance();
    await pickMode("Dark");
    await openEditorFromCurrent();
    await fill("Name", "QA Plum");
    await setHex("Accent, Dark", "#c04ad8");
    expect(await $(".theme-table__auto").getText()).toMatch(/auto/i);
    expect(await $('input[aria-label="Text, Light"]').getValue()).toMatch(/^#[0-9a-f]{6}$/i);
    expect(await $(".settings-warning").isExisting()).toBe(false);
    await setHex("Accent, Light", "#7a1d8f"); // light column is editable
    await $("button=Save as theme").click();
    await waitGone(".theme-editor");
    await expect($('.theme-picker__circle[aria-label^="QA Plum,"]')).toBeDisplayed();
    const darkAccent = await token("--color-accent");
    await pickMode("Light");
    await browser.waitUntil(async () => (await html()) === "light");
    expect(await token("--color-accent")).not.toBe(darkAccent);
    await pickMode("Dark");
    await closeSettings();
  });

  it("AC-2.13: Share puts an mct1. message in the chat; an official client sees the plain string", async () => {
    const official = new ScriptBot({ name: "OfficialBot", port: testenv.ports.v15 });
    try {
      await official.connected();
      await official.run("join Lobby");
      await appearance();
      await $('.theme-picker__circle[aria-label^="QA Plum,"]').click({ button: "right" });
      await $('[role="menuitem"]=Share').click();
      await closeSettings();
      const box = composer();
      await box.waitForEnabled();
      await box.click();
      await browser.keys(["Control", "v"]);
      code = (await box.getValue()).match(/mct1\.[A-Za-z0-9_-]+/)?.[0] ?? "";
      expect(code.length).toBeGreaterThan(20);
      expect(code.length).toBeLessThanOrEqual(2048);
      await browser.keys("Enter");
      const got = await official.run(`expect message --from ${USER} --contains "mct1."`);
      expect(got).toContain(code);
      await waitMessage("QA Plum");
      await expect($(".theme-card")).toBeDisplayed();
    } finally {
      official.stop();
    }
  });

  it("AC-2.13: a received card applies the theme; both versions equal the sender's", async () => {
    // Reset to Mumcord, then receive the code from a bot like a second client would.
    await appearance();
    await circle("Mumcord").click();
    await closeSettings();
    const sender = new ScriptBot({ name: "SenderBot", port: testenv.ports.v15 });
    try {
      await sender.connected();
      await sender.run("join Lobby");
      await sender.run(`say --channel Lobby "Theme: ${code}"`);
      await waitMessage("Theme:");
      const cards = await $$(".theme-card").getElements();
      const card = cards[cards.length - 1];
      await card.$("button.theme-card__apply").click();
      await browser.waitUntil(async () => (await token("--color-accent")) !== "");
      const darkAccent = await token("--color-accent");
      expect(darkAccent.toLowerCase()).toContain("c04ad8".slice(0, 2)); // plum accent applied
      await appearance();
      await pickMode("Light");
      await browser.waitUntil(async () => (await html()) === "light");
      await $('.theme-picker__circle[aria-label^="QA Plum,"]').click({ button: "right" });
      await $('[role="menuitem"]=Edit').click();
      expect(await $('input[aria-label="Accent, Light"]').getValue().then((v) => v.toLowerCase())).toBe("#7a1d8f");
      expect(await $('input[aria-label="Accent, Dark"]').getValue().then((v) => v.toLowerCase())).toBe("#c04ad8");
      await button("Cancel").click();
      await pickMode("System");
      await closeSettings();
    } finally {
      sender.stop();
    }
  });

  it("AC-2.13: an old single-set mct1. code gets the missing version generated and readable", async () => {
    const old = {
      v: "0.3.0",
      n: "Old Teal",
      c: { background: "#0b1d24", surface: "#12303b", text: "#e6f4f1", accent: "#2ec4b6", talking: "#3ddc84", warning: "#ffb020", error: "#ff5d5d" },
    };
    const oldCode = "mct1." + Buffer.from(JSON.stringify(old)).toString("base64url");
    const sender = new ScriptBot({ name: "OldBot", port: testenv.ports.v15 });
    try {
      await sender.connected();
      await sender.run("join Lobby");
      await sender.run(`say --channel Lobby "Old: ${oldCode}"`);
      await waitMessage("Old:");
      const cards = await $$(".theme-card").getElements();
      await cards[cards.length - 1].$("button.theme-card__apply").click();
      await appearance();
      await $('.theme-picker__circle[aria-label^="Old Teal,"]').click({ button: "right" });
      await $('[role="menuitem"]=Edit').click();
      expect(await $(".theme-table__auto").getText()).toMatch(/auto/i);
      expect(await $(".settings-warning").isExisting()).toBe(false);
      expect(await $("button=Save").isEnabled()).toBe(true);
      await button("Cancel").click();
      await closeSettings();
    } finally {
      sender.stop();
    }
  });

  it("AC-2.13/2.14: \"Apply to Server B\" sets B only; B shows it, A keeps the app theme", async () => {
    await $('button[aria-label="Add a server"]').click();
    await dialogTitle("Add a server").waitForDisplayed();
    await fill("Address", "127.0.0.1");
    await fill("Port", String(testenv.ports.v15b));
    await fill("Name in the server bar", SERVER_B);
    await fill("Your name on this server", "TesterB");
    await button("Add server").click();
    await serverButton(SERVER_B).waitForDisplayed();

    const appAccent = await token("--color-accent");
    const sender = new ScriptBot({ name: "CardBot", port: testenv.ports.v15 });
    try {
      await sender.connected();
      await sender.run("join Lobby");
      await sender.run(`say --channel Lobby "B: ${code}"`);
      await waitMessage("B:");
      const cards = await $$(".theme-card").getElements();
      const card = cards[cards.length - 1];
      await card.$("button.theme-card__arrow").click();
      await $(`[role="menuitem"]*=${SERVER_B}`).click();
    } finally {
      sender.stop();
    }
    // Still on A: app theme unchanged.
    expect(await token("--color-accent")).toBe(appAccent);
    await serverButton(SERVER_B).click({ button: "right" });
    await $('[role="menuitem"]=Theme…').click();
    await expect($(`.theme-picker__circle[aria-label^="QA Plum,"]`)).toBeDisplayed();
    expect(await $('.theme-picker__circle[aria-label*="selected"]').getAttribute("aria-label")).toContain("QA Plum");
    await button("Cancel").click();
  });
});
