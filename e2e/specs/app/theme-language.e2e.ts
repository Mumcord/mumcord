// AC-1.26: OS light -> light, OS dark -> dark (WebKitGTK prefers-color-scheme driven by GTK_THEME),
// manual override; language es/de switch live (no restart) and every UI text is translated;
// the picker offers all 24 EU languages by their own names (FR-1.36) and French/Greek apply live.
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { browser, expect, $ } from "@wdio/globals";
import { app, artifactsDir, bot, repoRoot, testenv } from "../../helpers/index.ts";
import {
  chooseRadio,
  connectFromEmptyState,
  htmlTheme,
  joinChannel,
  openSettings,
  settingsTab,
  waitFor,
  waitGone,
  wizard,
} from "./flows.ts";

const me = "C7Lang";
const TABS = 11;

/** Opens the settings tab called `name`; indexes shift whenever sections are added. */
async function settingsTabNamed(name: string): Promise<void> {
  for (let i = 0; i < TABS; i++) {
    if ((await settingsTab(i)) === name) return;
  }
  throw new Error(`settings tab "${name}" not found`);
}

/** All UI strings on screen: text nodes (incl. visually hidden), aria-labels, placeholders, titles. */
async function uiTexts(): Promise<string[]> {
  return browser.execute(() => {
    // User/server content, not UI text. Also skipped: license texts (legal originals, shown
    // verbatim) and the live regions (transient announcements made before the switch).
    const skip =
      ".chat-message__body,.chat-message__sender,.tree-person__name,.tree-channel__name,code,time,.server-bar__server,.chat-header__title,.profile-card__name,.member__name,pre.settings-text,[data-testid^='live-']";
    const out = new Set<string>();
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const p = n.parentElement;
      if (!p || p.closest(skip) || p.closest("script,style")) continue;
      const t = (n.textContent ?? "").trim();
      if (t) out.add(t);
    }
    for (const e of Array.from(document.querySelectorAll("[aria-label],[placeholder],[title],[aria-valuetext]"))) {
      if (e.closest(skip)) continue;
      for (const a of ["aria-label", "placeholder", "title", "aria-valuetext"]) {
        const v = e.getAttribute(a)?.trim();
        if (v) out.add(v);
      }
    }
    return Array.from(out);
  });
}

/** Texts of the main screen plus every settings section. */
async function collect(): Promise<Set<string>> {
  const all = new Set(await uiTexts());
  await openSettings();
  for (let i = 0; i < TABS; i++) {
    await settingsTab(i);
    await browser.pause(150);
    for (const t of await uiTexts()) all.add(t);
  }
  await browser.keys(["Escape"]);
  await waitGone(".overlay__dialog");
  return all;
}

// Same in every language: names, numbers, endonyms, units, symbols, product names.
// Game titles of the in-game overlay blocklist are names, not UI text.
const gameTitles = new Set(
  (
    JSON.parse(
      readFileSync(join(repoRoot, "crates/engine/src/ingame_overlay/blocklist.json"), "utf8"),
    ) as { games: { name: string }[] }
  ).games.map((g) => g.name),
);

const neutral = (t: string) =>
  gameTitles.has(t) ||
  /^[\d\s.,:%/#()+–\-·…★☆×▾▸○]*$/.test(t) ||
  // A single letter (avatar initial), the test audio backend's device name, the OS string.
  /^\p{L}$/u.test(t) ||
  /^(null|linux \S+|windows \S+|macos \S+)$/i.test(t) ||
  // "Beta" is the same word in English, German and Spanish.
  /^Beta$/.test(t) ||
  // "Link" is also the German word; its tooltip adds the shortcut.
  /^Link( \(Ctrl\+K\))?$/.test(t) ||
  // "In <channel> (<count>)" reads the same in English and German.
  /^In \S+ \(\d+\)$/.test(t) ||
  // Language names in the picker are shown in their own language (endonyms).
  /^(Български|Čeština|Dansk|Deutsch|Ελληνικά|English|Español|Eesti|Suomi|Français|Gaeilge|Hrvatski|Magyar|Italiano|Lietuvių|Latviešu|Malti|Nederlands|Polski|Português|Română|Slovenčina|Slovenščina|Svenska)$/.test(t) ||
  // Pixel values like "240 px" (and the bare unit) are not translated.
  /^(\d+\s*)?px$/.test(t) ||
  /^(Mumcord|Lobby|Raid|AFK|Root|Chat|Audio|Normal|Port|Version|System|Name|Overlay|Position|Support|C7Lang\w*|localhost.*|OK|Opus|UDP|TCP|TLS|mumble:\/\/.*|v?\d[\w.-]*|https?:\/\/.*|\w+@\w+.*)$/i.test(t);

describe("AC-1.26 theme follows OS + manual override", () => {
  before(async () => {
    await testenv.up("v15");
    delete process.env.GTK_THEME;
    await app.restart({ freshProfile: true });
    await wizard(me);
  });

  after(() => {
    delete process.env.GTK_THEME;
  });

  it("OS light -> Mumcord light", async () => {
    expect(await browser.execute(() => window.matchMedia("(prefers-color-scheme: dark)").matches)).toBe(false);
    expect(await htmlTheme()).toBe("light");
  });

  it("OS dark -> Mumcord dark", async () => {
    process.env.GTK_THEME = "Adwaita:dark";
    await app.restart();
    await waitFor(".shell", 30_000);
    expect(await browser.execute(() => window.matchMedia("(prefers-color-scheme: dark)").matches)).toBe(true);
    expect(await htmlTheme()).toBe("dark");
  });

  it("manual override Light wins over OS dark, live", async () => {
    await openSettings();
    await settingsTabNamed("Appearance");
    await chooseRadio("light");
    await browser.waitUntil(async () => (await htmlTheme()) === "light", { timeoutMsg: "override light not applied" });
    await browser.keys(["Escape"]);
  });

  it("manual override Dark wins over OS light and persists over restart", async () => {
    delete process.env.GTK_THEME;
    await app.restart();
    await waitFor(".shell", 30_000);
    expect(await htmlTheme()).toBe("light"); // stored override = light
    await openSettings();
    await settingsTabNamed("Appearance");
    await chooseRadio("dark");
    await browser.waitUntil(async () => (await htmlTheme()) === "dark", { timeoutMsg: "override dark not applied" });
    await chooseRadio("system");
    await browser.waitUntil(async () => (await htmlTheme()) === "light", { timeoutMsg: "back to system not applied" });
    await browser.keys(["Escape"]);
  });

  // FR-2.63: the menu is portalled to <body>, so it must stack above the settings overlay.
  it("right-click on a theme circle shows its menu above the settings", async () => {
    await openSettings();
    await $('[role="tab"]=Appearance').click();
    await $(".theme-picker__circle").click({ button: "right" });
    await waitFor('[role="menu"]');
    const onTop = await browser.execute(() => {
      const menu = document.querySelector('[role="menu"]');
      if (!menu) return false;
      const box = menu.getBoundingClientRect();
      const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
      return hit !== null && menu.contains(hit);
    });
    expect(onTop).toBe(true);
    const items = await browser.execute(() =>
      Array.from(document.querySelectorAll('[role="menu"] [role="menuitem"]')).map((item) => item.textContent),
    );
    expect(items).toEqual(["New theme from this", "Share"]);
    await browser.keys(["Escape"]);
    await waitGone('[role="menu"]');
    await browser.keys(["Escape"]);
    await waitGone(".overlay__dialog");
  });
});

describe("AC-1.26 language switch without restart", () => {
  let english = new Set<string>();
  const report: Record<string, string[]> = {};

  before(async () => {
    await connectFromEmptyState();
    await joinChannel("Lobby");
    await bot(["expect", "user", me, "--in", "Lobby"], { name: "C7LangPeer", timeout: 20 });
    english = await collect();
  });

  after(() => {
    writeFileSync(join(artifactsDir, "c7-untranslated.json"), JSON.stringify(report, null, 2));
  });

  for (const [lang, settingsTitle] of [
    ["es", "Ajustes"],
    ["de", "Einstellungen"],
  ] as const) {
    it(`switches to ${lang} live and translates every UI text`, async () => {
      const pid = await browser.execute(() => performance.timeOrigin);
      await openSettings();
      await settingsTabNamed("Language");
      await chooseRadio(lang);
      await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.lang)) === lang, {
        timeoutMsg: `html lang never became ${lang}`,
      });
      const title = await browser.execute(() => document.querySelector(".overlay__dialog h1")?.textContent ?? "");
      expect(title).toBe(settingsTitle);
      await browser.keys(["Escape"]);
      await waitGone(".overlay__dialog");
      const translated = await collect();
      // Same document (no reload / restart happened).
      expect(await browser.execute(() => performance.timeOrigin)).toBe(pid);
      const leftovers = [...translated].filter((t) => english.has(t) && !neutral(t));
      report[lang] = leftovers;
      if (leftovers.length) console.warn(`${lang}: untranslated candidates:\n  ${leftovers.join("\n  ")}`);
      expect(leftovers).toEqual([]);
    });
  }

  it("offers all 24 EU languages by their own names and switches to French and Greek live", async () => {
    const pid = await browser.execute(() => performance.timeOrigin);
    await openSettings();
    await settingsTabNamed("Language");
    const names = await browser.execute(() =>
      Array.from(document.querySelectorAll(".overlay__dialog input[type=radio]"), (r) => r.closest("label")?.textContent.trim() ?? ""),
    );
    expect(names).toEqual(expect.arrayContaining(["Français", "Ελληνικά", "Gaeilge", "Malti", "Български"]));
    expect(names).toHaveLength(25);
    for (const lang of ["fr", "el"]) {
      const strings = JSON.parse(readFileSync(join(repoRoot, `ui/src/locales/${lang}/app.json`), "utf8")) as {
        settings: { title: string };
      };
      await chooseRadio(lang);
      await browser.waitUntil(
        async () =>
          (await browser.execute(() => document.querySelector(".overlay__dialog h1")?.textContent ?? "")) ===
          strings.settings.title,
        { timeoutMsg: `settings title never became ${strings.settings.title}` },
      );
    }
    expect(await browser.execute(() => performance.timeOrigin)).toBe(pid);
    await browser.keys(["Escape"]);
    await waitGone(".overlay__dialog");
  });
});
