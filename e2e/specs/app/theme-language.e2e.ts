// AC-1.26: OS light -> light, OS dark -> dark (WebKitGTK prefers-color-scheme driven by GTK_THEME),
// manual override; language es/de switch live (no restart) and every UI text is translated;
// the picker offers all 24 EU languages by their own names (FR-1.36) and French/Greek apply live.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { browser, expect, $ } from "@wdio/globals";
import { app, bot, repoRoot, testenv } from "../../helpers/index.ts";
import {
  chooseRadio,
  connectFromEmptyState,
  focus,
  htmlTheme,
  joinChannel,
  openSettings,
  settingsTab,
  tabTo,
  waitFor,
  waitGone,
  wizard,
} from "./flows.ts";

const me = "C7Lang";
const TABS = 13;

/**
 * Opens the settings tab of `section` (its id, so it works in every UI language); indexes shift
 * whenever sections are added.
 */
async function settingsTabNamed(section: string): Promise<void> {
  for (let i = 0; i < TABS; i++) {
    await settingsTab(i);
    if ((await focus()).id.endsWith(`-tab-${section}`)) return;
  }
  throw new Error(`settings tab "${section}" not found`);
}

/** In Appearance, Tabs to the "Dark or light" dropdown (FR-2.63) and picks `label` by keyboard. */
async function chooseMode(label: string): Promise<void> {
  await tabTo((f) => f.role === "combobox" && f.id.endsWith("-mode"), "dark-or-light dropdown");
  await browser.keys(["Enter", "Home"]);
  for (let i = 0; i < 3; i++) {
    const active = await browser.execute(() => {
      const id = document.activeElement?.getAttribute("aria-activedescendant");
      return id ? (document.getElementById(id)?.textContent ?? "") : "";
    });
    if (active.trim() === label) break;
    await browser.keys(["ArrowDown"]);
  }
  await browser.keys(["Enter"]);
  await browser.waitUntil(async () => (await focus()).text === label, { timeoutMsg: `mode ${label} not chosen` });
}

/** All UI strings on screen: text nodes (incl. visually hidden), aria-labels, placeholders, titles. */
async function uiTexts(): Promise<string[]> {
  return browser.execute(() => {
    // User/server content, not UI text. Also skipped: license texts (legal originals, shown
    // verbatim), the live regions (transient announcements made before the switch), key chips
    // and the server name and figures ("1 online · 25 ms" reads the same in many languages).
    const skip =
      ".chat-message__body,.chat-message__sender,.tree-person__name,.tree-channel__name,code,time,.server-bar__server,.chat-header__title,.profile-card__name,.member__name,pre.settings-text,[data-testid^='live-'],kbd,.server-section__name,.server-section__meta";
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
  /^[\d\s.,:%/#()+–\-·…★☆×▾▸○?]*$/.test(t) ||
  // A single letter (avatar initial), the test audio backend's device name, the OS string.
  /^\p{L}$/u.test(t) ||
  /^(null|linux \S+|windows \S+|macos \S+)$/i.test(t) ||
  // "Beta" is the same word in English, German and Spanish; "Details" is the German one.
  /^(Beta|Details)$/.test(t) ||
  // "Apps" is also the German and Spanish word, "Bitrate" and "Updates" the German ones; preset
  // theme names are names, not translated.
  /^(Apps|Bitrate|Updates|Deep Ocean|Forest|Paper)$/.test(t) ||
  // "Standard" is the German default-identity badge ("Default"), which only matches the English
  // noise suppression level by chance.
  /^Standard$/.test(t) ||
  // "Link" is also the German word; its tooltip adds the shortcut.
  /^Link( \(Ctrl\+K\))?$/.test(t) ||
  // "In <channel> (<count>)" reads the same in English and German.
  /^In \S+ \(\d+\)$/.test(t) ||
  // Language names in the picker are shown in their own language (endonyms).
  /^(Български|Čeština|Dansk|Deutsch|Ελληνικά|English|Español|Eesti|Suomi|Français|Gaeilge|Hrvatski|Magyar|Italiano|Lietuvių|Latviešu|Malti|Nederlands|Polski|Português|Română|Slovenčina|Slovenščina|Svenska)$/.test(t) ||
  // Dev builds show their version as dev~<commit> or dev~YYYYMMDD (RELEASING.md).
  /^dev~[\w.-]+$/.test(t) ||
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
    await settingsTabNamed("appearance");
    await chooseMode("Light");
    await browser.waitUntil(async () => (await htmlTheme()) === "light", { timeoutMsg: "override light not applied" });
    await browser.keys(["Escape"]);
  });

  it("manual override Dark wins over OS light and persists over restart", async () => {
    delete process.env.GTK_THEME;
    await app.restart();
    await waitFor(".shell", 30_000);
    expect(await htmlTheme()).toBe("light"); // stored override = light
    await openSettings();
    await settingsTabNamed("appearance");
    await chooseMode("Dark");
    await browser.waitUntil(async () => (await htmlTheme()) === "dark", { timeoutMsg: "override dark not applied" });
    await chooseMode("Follow system");
    await browser.waitUntil(async () => (await htmlTheme()) === "light", { timeoutMsg: "back to system not applied" });
    await browser.keys(["Escape"]);
  });

  // FR-2.63: the menu is portalled to <body>, so it must stack above the settings overlay.
  it("right-click on a theme circle shows its menu above the settings", async () => {
    await openSettings();
    await $('[role="tab"]=Appearance').click();
    // A script contextmenu event, not a WebDriver right-click: after that, WebKitWebDriver types
    // ":" as ";" for the rest of the session, which breaks the next spec's server address.
    await browser.execute(() => {
      const circle = document.querySelector(".theme-picker__circle");
      if (!circle) throw new Error("no theme circle");
      const box = circle.getBoundingClientRect();
      circle.dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2, clientX: box.left + 4, clientY: box.top + 4 }),
      );
    });
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

  before(async () => {
    await connectFromEmptyState();
    await joinChannel("Lobby");
    await bot(["expect", "user", me, "--in", "Lobby"], { name: "C7LangPeer", timeout: 20 });
    english = await collect();
  });

  for (const [lang, settingsTitle] of [
    ["es", "Ajustes"],
    ["de", "Einstellungen"],
  ] as const) {
    it(`switches to ${lang} live and translates every UI text`, async () => {
      const pid = await browser.execute(() => performance.timeOrigin);
      await openSettings();
      await settingsTabNamed("language");
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
      if (leftovers.length) console.warn(`${lang}: untranslated candidates:\n  ${leftovers.join("\n  ")}`);
      expect(leftovers).toEqual([]);
    });
  }

  it("offers all 24 EU languages by their own names and switches to French and Greek live", async () => {
    const pid = await browser.execute(() => performance.timeOrigin);
    await openSettings();
    await settingsTabNamed("language");
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
