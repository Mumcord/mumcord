// AC-1.27: main flows keyboard-only (wizard, connect, join, chat, follow, settings) + axe WCAG 2.2 AA
// scan of every main screen, in the OS light theme and again in the OS dark theme. Zero violations.
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { browser, expect } from "@wdio/globals";
import { app, artifactsDir, bot, botInBackground, testenv } from "../../helpers/index.ts";
import {
  closeWindowLikeUser,
  connectFromEmptyState,
  findings,
  findingsReport,
  focus,
  focusChat,
  focusComposer,
  focusTree,
  htmlTheme,
  joinChannel,
  openSettings,
  scan,
  tabTo,
  typeText,
  treeTo,
  waitFor,
  waitGone,
  wizard,
} from "./flows.ts";

/**
 * The searchable language picker on the welcome step: opens with ArrowDown, filters while typing
 * (English name), gets scanned while open, and Escape closes it without choosing anything.
 */
async function languagePickerByKeyboard(theme: string): Promise<void> {
  await tabTo((f) => f.role === "combobox" && f.cls.includes("select__input"), "language picker");
  const state = () =>
    browser.execute(() => {
      const field = document.activeElement as HTMLInputElement;
      const list = document.getElementById(field.getAttribute("aria-controls") ?? "");
      return {
        expanded: field.getAttribute("aria-expanded"),
        value: field.value,
        options: Array.from(list?.querySelectorAll("[role=option]") ?? [], (o) => o.textContent.trim()),
        active: field.getAttribute("aria-activedescendant") ?? "",
        firstId: list?.querySelector("[role=option]")?.id ?? "",
      };
    });
  const before = await state();
  await browser.keys(["ArrowDown"]);
  expect((await state()).options).toHaveLength(25);
  await typeText("germ");
  await browser.waitUntil(async () => (await state()).options.join() === "Deutsch", {
    timeoutMsg: "typing 'germ' did not filter the language list to Deutsch",
  });
  const open = await state();
  expect(open.expanded).toBe("true");
  expect(open.active).toBe(open.firstId);
  await scan(`${theme} wizard language picker open`);
  await browser.keys(["Escape"]);
  const closed = await state();
  expect(closed.expanded).toBe("false");
  expect(closed.value).toBe(before.value);
  await waitFor("form.wizard-step");
}

/** Top-level settings groups (#161). */
const SETTINGS_TABS = 10;

for (const theme of ["light", "dark"] as const) {
  describe(`AC-1.27 keyboard-only flows + axe (${theme} OS theme)`, () => {
    const me = theme === "light" ? "C7KbdL" : "C7KbdD";
    const peer = `${me}Peer`;

    before(async () => {
      await testenv.up("v15");
      if (theme === "dark") process.env.GTK_THEME = "Adwaita:dark";
      else delete process.env.GTK_THEME;
      await app.restart({ freshProfile: true });
    });

    after(() => {
      delete process.env.GTK_THEME;
    });

    it("follows the OS theme on first start", async () => {
      await waitFor("form.wizard-step");
      expect(await htmlTheme()).toBe(theme);
    });

    it("wizard with keyboard only", async () => {
      // Custom walks every step of the wizard (#159), so each one gets scanned.
      await wizard(me, {
        style: "custom",
        onStep: async (step) => {
          await scan(`${theme} wizard step ${step}`);
          if (step === "welcome") await languagePickerByKeyboard(theme);
        },
      });
      await scan(`${theme} main empty state`);
    });

    it("connect with keyboard only (empty state + certificate dialog)", async () => {
      await connectFromEmptyState(async () => {
        await scan(`${theme} certificate trust dialog`);
      });
      await bot(["expect", "user", me], { name: peer, timeout: 20 });
      await scan(`${theme} main connected`);
    });

    it("join a channel with keyboard only", async () => {
      await joinChannel("Lobby");
      await bot(["expect", "user", me, "--in", "Lobby"], { name: peer, timeout: 20 });
    });

    it("chat with keyboard only (send + receive)", async () => {
      const listener = botInBackground(["script"], { name: peer, timeout: 30 });
      listener.send("join Lobby");
      listener.send(`expect message --from ${me} --contains kbd-hello-${theme}`);
      listener.send(`say "reply from peer ${theme}"`);
      listener.send("sleep 2");
      listener.process.stdin?.end();
      // Let the bot reach Lobby before we send.
      await browser.pause(2500);
      await focusChat();
      await focusComposer();
      await typeText(`kbd-hello-${theme}`);
      await browser.keys(["Enter"]);
      await listener.done;
      await waitFor(`.chat-message`);
      await scan(`${theme} chat with messages`);
      // Message list is keyboard reachable.
      await tabTo((f) => f.cls.includes("chat-message"), "message list item", true);
    });

    it("follow a channel with keyboard only", async () => {
      await focusTree();
      await treeTo("Raid");
      await browser.keys(["Enter"]);
      const toggle = await tabTo((f) => f.cls.includes("follow-toggle"), "Follow toggle");
      expect(toggle.label).toBe("Follow channel");
      await browser.keys(["Enter"]);
      await bot(["expect", "user", me, "--listening", "Raid"], { name: peer, timeout: 20 });
      await scan(`${theme} chat followed channel`);
    });

    it("add-server dialog with keyboard only", async () => {
      await tabTo((f) => f.cls.includes("server-bar__add"), "Add a server button");
      await browser.keys(["Enter"]);
      await waitFor("dialog[open] form");
      await scan(`${theme} add server dialog`);
      await tabTo((f) => f.tag === "input" && f.type !== "checkbox", "first dialog field");
      await browser.keys(["Escape"]);
      await waitGone("dialog[open] form");
    });

    it("settings with keyboard only, every section", async () => {
      await openSettings();
      for (let i = 0; i < SETTINGS_TABS; i++) {
        await browser.keys([i === 0 ? "Home" : "ArrowDown"]);
        const tab = (await focus()).text;
        await scan(`${theme} settings ${String(i)} ${tab}`);
      }
      await browser.keys(["Escape"]);
      await waitGone(".overlay__dialog");
    });

    it("close dialog (scan) and cancel with Escape", async () => {
      await closeWindowLikeUser();
      await waitFor("dialog.app-dialog[open]");
      await scan(`${theme} close dialog`);
      await browser.keys(["Escape"]);
      await waitGone("dialog.app-dialog[open]");
      await bot(["expect", "user", me], { name: peer });
    });

    it("theme stayed on the OS theme throughout", async () => {
      expect(await htmlTheme()).toBe(theme);
    });
  });
}

describe("AC-1.27 axe gate", () => {
  it("has zero WCAG 2.2 AA violations on all scanned screens", () => {
    const report = findingsReport();
    writeFileSync(join(artifactsDir, "c7-axe-findings.json"), JSON.stringify(findings, null, 2));
    writeFileSync(join(artifactsDir, "c7-axe-findings.txt"), report);
    if (findings.length > 0) console.warn(`axe findings:\n${report}`);
    expect(findings.length).toBe(0);
  });
});
