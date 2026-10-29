import { readFileSync } from "node:fs";
import { browser, expect, $, $$ } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  finishWizard,
  installOpenerStub,
  joinChannel,
  messageBodies,
  pasteImage,
  composer,
  ScriptBot,
  selectChannel,
  sendText,
  sleep,
  USER,
  waitMessage,
} from "../../helpers/chat-ui.ts";

// AC-1.13 (rendering + sanitizing + links) and AC-1.14 (sending Markdown and pasted images).
describe("chat: rendering and sending (AC-1.13, AC-1.14, AC-1.43)", () => {
  let opener: { log: string; binDir: string };

  before(async () => {
    await testenv.up("v15");
    // Fake system browser: the app inherits PATH from tauri-driver, so restart after setting it.
    opener = installOpenerStub();
    process.env.PATH = `${opener.binDir}:${process.env.PATH ?? ""}`;
    await app.restart();
    await finishWizard();
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
    await selectChannel("Lobby");
  });

  it("AC-1.13 shows bold text, a link and an inline image formatted", async () => {
    await bot(["join", "Lobby"], { name: "RenderBot", hold: 1 });
    await bot(["say-sample", "--channel", "Lobby", "bold"], { name: "RenderBot", hold: 1 });
    await bot(["say-sample", "--channel", "Lobby", "link"], { name: "RenderBot", hold: 1 });
    await bot(["say-sample", "--channel", "Lobby", "image"], { name: "RenderBot", hold: 1 });
    await waitMessage("bold text");
    await waitMessage("mumble.info");
    await browser.waitUntil(async () => (await messageBodies()).some((b) => b.includes("<img")), {
      timeoutMsg: "image message not shown",
    });
    const bold = $("//li[contains(@class,'chat-message')]//b[.='bold text']");
    await expect(bold).toExist();
    const link = $("//li[contains(@class,'chat-message')]//a[@href='https://www.mumble.info/']");
    await expect(link).toExist();
    const img = $("//li[contains(@class,'chat-message')]//img[starts-with(@src,'data:image/png;base64,')]");
    await expect(img).toBeDisplayed();
    // Actually decoded (rendered), not a broken image.
    const width = await browser.execute(
      () => document.querySelector<HTMLImageElement>(".chat-list__items img")?.naturalWidth ?? 0,
    );
    expect(width).toBeGreaterThan(0);
  });

  it("AC-1.13 neither runs a <script> nor loads an external image", async () => {
    await browser.execute(() => {
      (window as unknown as { __xss: number }).__xss = 0;
      window.alert = () => {
        (window as unknown as { __xss: number }).__xss++;
      };
    });
    await bot(["say-sample", "--channel", "Lobby", "script"], { name: "RenderBot", hold: 1 });
    await bot(["say-sample", "--channel", "Lobby", "external-image"], { name: "RenderBot", hold: 1 });
    await waitMessage("beforeafter");
    await waitMessage("remote image:");
    await sleep(1000);
    const result = await browser.execute(() => ({
      scripts: document.querySelectorAll(".chat-list__items script").length,
      externalImgs: Array.from(document.querySelectorAll(".chat-list__items img")).filter(
        (i) => !(i.getAttribute("src") ?? "").startsWith("data:"),
      ).length,
      requested: performance
        .getEntriesByType("resource")
        .filter((e) => e.name.includes("tracker.example.invalid")).length,
      xss: (window as unknown as { __xss: number }).__xss,
    }));
    expect(result).toEqual({ scripts: 0, externalImgs: 0, requested: 0, xss: 0 });
  });

  it("AC-1.13 clicking the link hands it to the system browser", async () => {
    const link = $("//li[contains(@class,'chat-message')]//a[@href='https://www.mumble.info/']");
    await link.click();
    await browser.waitUntil(() => readFileSync(opener.log, "utf8").includes("https://www.mumble.info/"), {
      timeout: 10_000,
      timeoutMsg: "system opener was not invoked with the link",
    });
    // The webview itself must not navigate away.
    await expect($(".chat-list__items")).toBeDisplayed();
  });

  it("AC-1.14 sends **bold** and a pasted image as official-client HTML", async () => {
    const listener = new ScriptBot({ name: "SendBot" });
    try {
      await listener.connected();
      await listener.run("join Lobby");
      await pasteImage(32, 32, false);
      await $("//*[starts-with(@alt,'Image to send')]").waitForExist({ timeout: 10_000 });
      await sendText("**bold** words");
      // The text and the image are separate messages (the server limit is per message), text first.
      const text = await listener.run(`expect message --from ${USER} --contains "words"`);
      expect(text).toMatch(/<(b|strong)>bold<\/(b|strong)>/);
      const got = await listener.run(`expect message --from ${USER} --contains "<img"`);
      expect(got).toMatch(/<img src="data:image\/(png|jpeg);base64,/);
    } finally {
      listener.stop();
    }
  });

  it("AC-1.43 a code block from Ctrl+Shift+E renders highlighted; the bot gets plain <pre><code>", async () => {
    const listener = new ScriptBot({ name: "CodeBot" });
    try {
      await listener.connected();
      await listener.run("join Lobby");
      const box = composer();
      await box.waitForEnabled();
      await box.setValue("const answer = 42; function show() { return answer; }");
      await browser.keys(["Control", "a"]);
      await browser.keys(["Control", "Shift", "e"]);
      expect(await box.getValue()).toBe(
        "```\nconst answer = 42; function show() { return answer; }\n```",
      );
      await browser.keys("Enter");
      const got = await listener.run(`expect message --from ${USER} --contains "<pre>"`);
      expect(got).toMatch(/<pre><code>const answer = 42;/);
      expect(got).not.toContain("hljs");
      await waitMessage("const answer = 42;");
      const own = (await messageBodies()).find((body) => body.includes("answer"));
      expect(own).toMatch(/<code class="hljs">/);
      expect(own).toMatch(/<span class="hljs-keyword">const<\/span>/);
      // Issue #111: the block carries its copy and download buttons.
      await expect($('.chat-code button[aria-label="Copy code"]')).toExist();
      await expect($('.chat-code button[aria-label="Download code"]')).toExist();
    } finally {
      listener.stop();
    }
  });

  it("issue #112 a ```sql block keeps its language through the server", async () => {
    const listener = new ScriptBot({ name: "LangBot" });
    try {
      await listener.connected();
      await listener.run("join Lobby");
      const box = composer();
      await box.waitForEnabled();
      // A typed newline would send the message, so the fence comes from Ctrl+Shift+E.
      await box.setValue("SELECT name FROM users;");
      await browser.keys(["Control", "a"]);
      await browser.keys(["Control", "Shift", "e"]);
      await browser.keys(["Control", "Home"]);
      await browser.keys(["End"]);
      await browser.keys("sql");
      expect(await box.getValue()).toBe("```sql\nSELECT name FROM users;\n```");
      await browser.keys("Enter");
      const got = await listener.run(`expect message --from ${USER} --contains "SELECT name"`);
      expect(got).toContain('<pre data-mumcord-lang="sql"><code>SELECT name FROM users;');
      await waitMessage("SELECT name FROM users;");
      const label = await $(".chat-code__label=SQL");
      await expect(label).toExist();
    } finally {
      listener.stop();
    }
  });

  it("AC-1.14 compresses (or refuses with a message) an image above the server limit", async () => {
    const listener = new ScriptBot({ name: "SendBot2" });
    try {
      await listener.connected();
      await listener.run("join Lobby");
      // 2000x2000 random noise ≈ 16 MB as PNG; v15's image limit is 128 KiB.
      await pasteImage(2000, 2000, true);
      const preview = $("//*[starts-with(@alt,'Image to send')]");
      const refused = $("//*[@role='alert' and contains(.,'too large')]");
      await browser.waitUntil(async () => (await preview.isExisting()) || (await refused.isExisting()), {
        timeout: 30_000,
        timeoutMsg: "neither a compressed preview nor a refusal",
      });
      if (await refused.isExisting()) {
        console.warn(`AC-1.14 oversized image refused: ${await refused.getText()}`);
        return;
      }
      await sendText("big");
      const got = await listener.run(`expect message --from ${USER} --contains "<img"`);
      const html = got.trim();
      console.warn(`AC-1.14 oversized image compressed to ${String(html.length)} chars`);
      expect(html.length).toBeLessThanOrEqual(131_072);
      expect(html).toContain("data:image/jpeg;base64,");
    } finally {
      listener.stop();
    }
  });

  // FR-1.48b / QA.15: pinned channel description and welcome text at the top of the chat.
  describe("pinned channel info (FR-1.48b)", () => {
    const region = () => $("//section[@aria-label='Channel info']");
    // Long enough (≥ 128 bytes) that the server only announces it by hash.
    const long =
      "<p><b>Raid rules</b>: be on time.</p><script>alert(1)</script>" +
      `<p>${"Bring potions and food. ".repeat(12)}</p><p>Last line of the rules.</p>`;

    after(async () => {
      await bot(["describe", "Raid", ""], { superuser: true }).catch(() => undefined);
    });

    it("fetches a long description on demand, collapsed, sanitized and live-updating", async () => {
      await bot(["describe", "Raid", long], { superuser: true });
      await selectChannel("Raid");
      await browser.waitUntil(async () => (await region().getText()).includes("Raid rules"), {
        timeoutMsg: "description not pinned",
      });
      expect(await region().$$("script").length).toBe(0);
      await expect(region().$("b=Raid rules")).toExist();
      const more = region().$("button=Show more");
      await expect(more).toHaveAttribute("aria-expanded", "false");
      await more.click();
      await expect(region().$("button=Show less")).toHaveAttribute("aria-expanded", "true");

      await bot(["describe", "Raid", "Short news"], { superuser: true });
      await browser.waitUntil(async () => (await region().getText()).includes("Short news"), {
        timeoutMsg: "changed description not shown",
      });
      await bot(["describe", "Raid", ""], { superuser: true });
      await region().waitForExist({ reverse: true, timeoutMsg: "removed description still shown" });
    });

    it("pins the welcome text in the root channel only", async () => {
      await selectChannel("Lobby");
      await expect(region()).not.toExist();
      // The root is the first channel in the tree, shown under the server's registered name.
      const channels = await $$("//span[contains(@class,'tree-channel__name')]");
      await channels[0].click();
      await browser.waitUntil(
        async () => (await region().isExisting()) && (await region().getText()).includes("Welcome to this server"),
        { timeoutMsg: "welcome text not pinned in the root channel" },
      );
    });
  });

  // Issue #114. The clipboard is read back by pasting into the message box with a real Ctrl+V.
  it("issue #114 copies selected message text from the context menu and with Ctrl+C", async () => {
    await selectChannel("Lobby");
    await sendText("select these words please");
    await waitMessage("select these words please");
    /** Selects `words` in the newest message; returns false if they are not there. */
    const selectWords = (words: string) =>
      browser.execute((words) => {
        const bodies = document.querySelectorAll(".chat-list__items .chat-message__body");
        const body = bodies[bodies.length - 1];
        const walker = document.createTreeWalker(body, NodeFilter.SHOW_TEXT);
        for (let node = walker.nextNode(); node; node = walker.nextNode()) {
          const at = (node.textContent ?? "").indexOf(words);
          if (at < 0) continue;
          const range = document.createRange();
          range.setStart(node, at);
          range.setEnd(node, at + words.length);
          window.getSelection()?.removeAllRanges();
          window.getSelection()?.addRange(range);
          return true;
        }
        return false;
      }, words);
    const pasted = async () => {
      const box = composer();
      await box.click();
      await box.clearValue();
      await browser.keys(["Control", "v"]);
      await browser.waitUntil(async () => (await box.getValue()) !== "", {
        timeoutMsg: "nothing pasted from the clipboard",
      });
      const value = await box.getValue();
      await box.clearValue();
      return value;
    };

    expect(await selectWords("these words")).toBe(true);
    // A script contextmenu event, not a WebDriver right-click (see theme-language.e2e.ts).
    await browser.execute(() => {
      const items = document.querySelectorAll(".chat-list__items > li");
      const item = items[items.length - 1];
      const box = item.getBoundingClientRect();
      item.dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2, clientX: box.left + 8, clientY: box.top + 8 }),
      );
    });
    await $('[role="menu"]').waitForDisplayed();
    const items = await browser.execute(() =>
      Array.from(document.querySelectorAll('[role="menu"] [role="menuitem"]')).map((item) => item.textContent),
    );
    expect(items[0]).toBe("Copy");
    await $('[role="menuitem"]=Copy').click();
    await $("p.chat-link-copied=Text copied").waitForExist();
    expect(await pasted()).toBe("these words");

    await browser.execute(() => {
      const items = document.querySelectorAll<HTMLElement>(".chat-list__items > li");
      items[items.length - 1].focus();
    });
    // A different part, so the check can't pass on what the menu copied before.
    expect(await selectWords("words please")).toBe(true);
    await browser.keys(["Control", "c"]);
    expect(await pasted()).toBe("words please");
  });

  // Issue #115. The copied image is read back by pasting it into the message box with Ctrl+V.
  it("issue #115 copies a chat image from its context menu as image data", async () => {
    await selectChannel("Lobby");
    // The PNG RenderBot sent in AC-1.13; a script contextmenu event (see theme-language.e2e.ts).
    await browser.execute(() => {
      const image = document.querySelector<HTMLImageElement>(".chat-list__items .chat-message__body img");
      if (image === null) throw new Error("no message image");
      image.scrollIntoView({ block: "center" });
      const box = image.getBoundingClientRect();
      image.dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2, clientX: box.left + 4, clientY: box.top + 4 }),
      );
    });
    await $('[role="menu"]').waitForDisplayed();
    const items = await browser.execute(() =>
      Array.from(document.querySelectorAll('[role="menu"] [role="menuitem"]')).map((item) => item.textContent),
    );
    expect(items).toEqual(["Copy image", "Save image…", "Open in image viewer"]);
    await $('[role="menuitem"]=Copy image').click();
    await $("p.chat-link-copied=Image copied").waitForExist();

    const box = composer();
    await box.click();
    await browser.keys(["Control", "v"]);
    const attached = $(".chat-tray__item");
    await attached.waitForExist({ timeoutMsg: "the copied image was not pasted" });
    await $(".chat-tray__remove").click();
    await attached.waitForExist({ reverse: true });
  });

  it("keeps the window fixed when the chat overflows (no document scroll)", async () => {
    await selectChannel("Lobby");
    for (let i = 0; i < 40; i++) await sendText(`overflow line ${String(i)}`);
    await waitMessage("overflow line 39");
    await sleep(500);
    const m = await browser.execute(() => {
      const d = document.scrollingElement as HTMLElement;
      const list = document.querySelector<HTMLElement>(".chat-list");
      return {
        docH: d.scrollHeight,
        winH: window.innerHeight,
        docW: d.scrollWidth,
        winW: window.innerWidth,
        top: d.scrollTop,
        left: d.scrollLeft,
        listScrolls: !!list && list.scrollHeight > list.clientHeight,
      };
    });
    expect(m.docH).toBe(m.winH);
    expect(m.docW).toBe(m.winW);
    expect(m.top).toBe(0);
    expect(m.left).toBe(0);
    expect(m.listScrolls).toBe(true);
  });
});
