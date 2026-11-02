// AC-2.12–AC-2.16: theme presets, Dark / Light / System versions, the Light | Dark editor with its
// contrast gate and auto-generated version, sharing (mct1. codes, theme card, "Apply to" a server),
// per-server themes, visual effects and the themed dropdowns. The second Mumcord client of AC-2.13
// is played by the app itself (receiver) and by the bot (sender / official client).
import { browser, expect, $, $$ } from "@wdio/globals";
import { app, testenv } from "../../helpers/index.ts";
import { hasClass } from "../../helpers/selectors.ts";
import {
  ScriptBot,
  USER,
  composer,
  joinChannel,
  readClipboard,
  waitMessage,
} from "../../helpers/chat-ui.ts";
import { button, completeWizard, dialogTitle, fill } from "../../helpers/servers-ui.ts";
import { connectFromEmptyState, waitFor, waitGone } from "./flows.ts";

const SERVER_B = "Server B";

const serverB = () => $("(//ul[@aria-label='Saved servers']//button[@aria-label])[2]");
const html = () => browser.execute(() => document.documentElement.dataset.theme ?? "");
const token = (name: string) =>
  browser.execute((n: string) => getComputedStyle(document.documentElement).getPropertyValue(n).trim(), name);
const chatBg = () => token("--color-bg-chat");

async function appearance(): Promise<void> {
  if (!(await $(".overlay__dialog").isExisting())) {
    await $('button[aria-label="Quick settings"]').click();
    await waitFor(".quick-settings");
    await $("//*[contains(@class,'quick-settings')]//button[contains(normalize-space(.),'All settings')]").click();
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
  await waitFor(".theme-editor");
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
  });

  // A failed test must not leave a dialog open that blocks every later one.
  beforeEach(async () => {
    for (let i = 0; i < 4; i++) {
      if (!(await $(".overlay__dialog, dialog[open], .theme-preview-bar").isExisting())) return;
      await browser.keys(["Escape"]);
      await browser.pause(200);
    }
  });

  it("setup: wizard and first server", async () => {
    await app.restart({ freshProfile: true });
    await completeWizard(USER);
    await connectFromEmptyState(undefined, testenv.address("v15"));
    await joinChannel("Lobby");
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
    await pickMode("Follow system");
    await browser.waitUntil(async () => (await html()) === "light"); // OS is light in the harness
    await closeSettings();
  });

  it("AC-2.12: a custom theme made in dark gets a passing light version, both editable", async () => {
    await appearance();
    await pickMode("Dark");
    await openEditorFromCurrent();
    await $(".theme-editor__name").setValue("QA Plum");
    await setHex("Accent, Dark", "#c04ad8");
    // "New theme from this" copies both hand-made versions, so nothing is marked "auto" here; the
    // light column is present, readable and editable.
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
      // The share message goes to the system clipboard through the engine; read it there (the
      // webview's own paste does not see it everywhere) and type it into the composer.
      await browser.waitUntil(async () => (await readClipboard("text")).includes("mct1."), {
        timeout: 10_000,
        timeoutMsg: "the share message did not reach the system clipboard",
      });
      const shared = await readClipboard("text");
      // setValue types key by key and loses the capital letters of the code; fill the field whole.
      await browser.execute((text: string) => {
        const field = document.querySelector<HTMLTextAreaElement>("textarea[aria-label^='Message']");
        if (!field) throw new Error("no composer");
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set?.call(
          field,
          text,
        );
        field.dispatchEvent(new Event("input", { bubbles: true }));
      }, shared);
      code = shared.match(/mct1\.[A-Za-z0-9_-]+/)?.[0] ?? "";
      expect(code.length).toBeGreaterThan(20);
      expect(code.length).toBeLessThanOrEqual(2048);
      await browser.keys("Enter");
      const got = await official.run(`expect message --from ${USER} --contains "mct1."`);
      expect(got).toContain(code);
      await waitMessage("QA Plum");
      // Several theme cards are listed here, so the check is about the list, not one card.
      await expect($$(".theme-card")).toBeDisplayed();
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
      // Read before the click: the theme can apply before a read right after it.
      const before = await token("--color-accent");
      await card.$("button.theme-card__apply").click();
      await browser.waitUntil(async () => (await token("--color-accent")) !== before, {
        timeoutMsg: "received theme was not applied",
      });
      const darkAccent = await token("--color-accent");
      expect(darkAccent.toLowerCase()).toMatch(/c04ad8|7a1d8f/); // plum accent applied (dark or light version)
      await appearance();
      await pickMode("Light");
      await browser.waitUntil(async () => (await html()) === "light");
      await $('.theme-picker__circle[aria-label^="QA Plum,"]').click({ button: "right" });
      await $('[role="menuitem"]=Edit theme').click();
      expect(await $('input[aria-label="Accent, Light"]').getValue().then((v) => v.toLowerCase())).toBe("#7a1d8f");
      expect(await $('input[aria-label="Accent, Dark"]').getValue().then((v) => v.toLowerCase())).toBe("#c04ad8");
      await button("Cancel").click();
      await pickMode("Follow system");
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
      await $('[role="menuitem"]=Edit theme').click();
      expect(await $(".theme-table__auto").getText()).toMatch(/auto/i);
      expect(await $(".settings-warning").isExisting()).toBe(false);
      expect(await $("button=Save theme").isEnabled()).toBe(true);
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
    await serverB().waitForDisplayed();

    // The app theme must differ from the shared one, or "B keeps its own theme" proves nothing.
    await appearance();
    await circle("Mumcord").click();
    await closeSettings();
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
      await $(
        `//*[@role='menuitem'][contains(translate(., 'ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'abcdefghijklmnopqrstuvwxyz'), '${SERVER_B.toLowerCase()}')]`,
      ).click();
    } finally {
      sender.stop();
    }
    // Still on A: app theme unchanged.
    expect(await token("--color-accent")).toBe(appAccent);
    await serverB().click({ button: "right" });
    // The right-click also reaches B's connect handler, which asks to trust B's certificate; accept it
    // (the next test needs B connected anyway) and reopen the menu.
    const trust = button("Trust and connect");
    if (await trust.waitForDisplayed({ timeout: 5_000 }).catch(() => false)) {
      await trust.click();
      await waitGone("dialog[open]");
      await serverB().click({ button: "right" });
    }
    await $('[role="menuitem"]=Theme…').click();
    await expect($(`.theme-picker__circle[aria-label^="QA Plum,"]`)).toBeDisplayed();
    expect(await $('.theme-picker__circle[aria-label*="selected"]').getAttribute("aria-label")).toContain("QA Plum");
    await button("Cancel").click();
  });

  it("AC-2.14: B shows its own theme while displayed; switching back to A shows the app theme", async () => {
    const switchTo = async (server: ReturnType<typeof serverB>) => {
      await server.click();
      for (const label of ["Disconnect and switch", "Trust and connect"]) {
        const b = $(`//button[normalize-space(.)='${label}']`);
        if (await b.waitForDisplayed({ timeout: 3_000 }).catch(() => false)) await b.click();
      }
    };
    // B may already be the displayed server from the previous test, so read the app theme from A.
    await switchTo($("//ul[@aria-label='Saved servers']//button[starts-with(@aria-label,'localhost')]"));
    await waitFor(".theme-card");
    const appAccent = await token("--color-accent");
    await switchTo(serverB());
    await browser.waitUntil(async () => (await token("--color-accent")) !== appAccent, {
      timeoutMsg: "server B did not show its own theme",
    });
    await switchTo($("//ul[@aria-label='Saved servers']//button[starts-with(@aria-label,'localhost')]"));
    await browser.waitUntil(async () => (await token("--color-accent")) === appAccent, {
      timeoutMsg: "server A did not go back to the app theme",
    });
  });

  it("compact chat layout keeps the theme card's title clear of its thumbnail", async () => {
    // The layout is pure CSS keyed on this attribute, so switching it directly avoids the settings round trip.
    await browser.execute(() => {
      document.querySelector(".chat-list__items")?.setAttribute("data-display", "compact");
    });
    try {
      const gap = await browser.execute(() => {
        const card = document.querySelector(".theme-card");
        const mock = card?.querySelector(".theme-card__mock")?.getBoundingClientRect();
        const name = card?.querySelector(".theme-card__name")?.getBoundingClientRect();
        const bounds = card?.getBoundingClientRect();
        if (!mock || !name || !bounds) return null;
        return { title: name.left - mock.right, inside: name.right <= bounds.right && mock.left >= bounds.left };
      });
      expect(gap).not.toBeNull();
      expect(gap?.title).toBeGreaterThanOrEqual(0);
      expect(gap?.inside).toBe(true);
    } finally {
      await browser.execute(() => {
        document.querySelector(".chat-list__items")?.setAttribute("data-display", "cozy");
      });
    }
  });

  it("AC-2.15: primary buttons glow; the Visual effects switch turns glass and grain off", async () => {
    await appearance();
    await openEditorFromCurrent();
    const glow = await browser.execute(() => {
      const b = document.querySelector(".theme-editor .settings-button--primary");
      return b ? getComputedStyle(b).boxShadow : "";
    });
    expect(glow).not.toBe("none");
    expect(glow).not.toBe("");
    await button("Cancel").click();
    const read = () =>
      browser.execute(() => ({
        effects: document.documentElement.dataset.effects ?? "",
        grain: getComputedStyle(document.body, "::after").opacity,
        glass: getComputedStyle(document.documentElement).getPropertyValue("--glass-filter").trim(),
      }));
    const on = await read();
    expect(on.effects).toBe("on");
    expect(Number(on.grain)).toBeGreaterThan(0);
    expect(on.glass).not.toBe("none");
    const sw = $(`//div[${hasClass('settings-switch')}][.//*[starts-with(normalize-space(.),'Visual effects')]]//input[@role='switch']`);
    await sw.scrollIntoView();
    await sw.click();
    await browser.waitUntil(async () => (await read()).effects === "off");
    const off = await read();
    expect(Number(off.grain)).toBe(0);
    expect(off.glass).toBe("none");
    await sw.click();
    await browser.waitUntil(async () => (await read()).effects === "on");
    await closeSettings();
  });

  it("AC-2.16: dropdowns are the themed component, keyboard operable and announced", async () => {
    await appearance();
    const nativeSelects = async () => browser.execute(() => document.querySelectorAll("select").length);
    const combo = $("//label[normalize-space(.)='Dark or light']/following::*[@role='combobox'][1]");
    await combo.scrollIntoView();
    const attrs = await browser.execute(() => {
      const c = document.querySelector("[role='combobox']");
      return c ? [c.getAttribute("aria-haspopup"), c.getAttribute("aria-expanded"), c.id] : [];
    });
    expect(attrs.slice(0, 2)).toEqual(["listbox", "false"]);
    // Keyboard only: focus, open, move, choose, close.
    await browser.execute((id: string) => document.getElementById(id)?.focus(), attrs[2] as string);
    await browser.keys(["Enter"]);
    await waitFor("[role='listbox']");
    expect(await combo.getAttribute("aria-expanded")).toBe("true");
    await browser.keys(["ArrowDown", "Enter"]);
    await waitGone("[role='listbox']");
    expect(await combo.getAttribute("aria-expanded")).toBe("false");
    await browser.keys(["Enter"]);
    await waitFor("[role='listbox']");
    await browser.keys(["Escape"]);
    await waitGone("[role='listbox']");
    await waitFor(".overlay__dialog"); // Escape closed only the list
    // Every settings tab: no native <select>, every themed one has an accessible name.
    const names = await $$('[role="tab"]').map((t) => t.getText());
    const bad: string[] = [];
    for (const name of names) {
      await $(`[role="tab"]=${name}`).click();
      await browser.pause(150);
      if ((await nativeSelects()) > 0) bad.push(`${name}: native select`);
      const unnamed = await browser.execute(() =>
        Array.from(document.querySelectorAll("[role='combobox']")).filter((c) => {
          const labelled = c.getAttribute("aria-label") || c.getAttribute("aria-labelledby");
          const byFor = c.id && document.querySelector(`label[for="${CSS.escape(c.id)}"]`);
          return !labelled && !byFor;
        }).length,
      );
      if (unnamed > 0) bad.push(`${name}: ${String(unnamed)} unnamed combobox`);
    }
    expect(bad).toEqual([]);
    await closeSettings();
  });

  it("AC-2.16: controls sit right-aligned in every section card of every settings group", async () => {
    await appearance();
    const names = await $$('[role="tab"]').map((t) => t.getText());
    // The eleven top-level groups: the ten of #161 plus Plugins.
    expect(names).toHaveLength(11);
    const off: string[] = [];
    for (const name of names) {
      await $(`[role="tab"]=${name}`).click();
      await browser.pause(150);
      off.push(
        ...(await browser.execute((section: string) => {
          const panel = document.querySelector("[role='tabpanel']:not([hidden])") ?? document.body;
          const out: string[] = [];
          // Every page is a grid of section cards (#161); the check runs inside each of them.
          if (panel.querySelector(".settings-group") === null) out.push(`${section}: no section cards`);
          const sel = ".select, input.settings-input, input[role='switch'], textarea.settings-input";
          for (const el of Array.from(panel.querySelectorAll<HTMLElement>(sel))) {
            const r = el.getBoundingClientRect();
            if (r.width === 0 || r.height === 0) continue;
            const row = el.closest<HTMLElement>(".settings-control-row, .settings-switch");
            const label = (el.getAttribute("aria-label") ?? (el.id || el.className)).slice(0, 40);
            if (!el.closest(".settings-group")) out.push(`${section}: ${label} outside a section card`);
            if (!row) {
              out.push(`${section}: ${label} not in a right-aligned row (${el.parentElement?.className ?? ""})`);
              continue;
            }
            const gap = row.getBoundingClientRect().right - r.right;
            if (gap > 12) out.push(`${section}: ${label} (${row.className}) right gap ${String(Math.round(gap))}px`);
          }
          return out;
        }, name)),
      );
    }
    expect(off).toEqual([]);
    await closeSettings();
  });
});
