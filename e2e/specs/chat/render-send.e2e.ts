import { readFileSync } from "node:fs";
import { browser, expect, $ } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  finishWizard,
  installOpenerStub,
  joinChannel,
  messageBodies,
  pasteImage,
  ScriptBot,
  selectChannel,
  sendText,
  sleep,
  USER,
  waitMessage,
} from "../../helpers/chat-ui.ts";

// AC-1.13 (rendering + sanitizing + links) and AC-1.14 (sending Markdown and pasted images).
describe("chat: rendering and sending (AC-1.13, AC-1.14)", () => {
  let opener: { log: string; binDir: string };

  before(async () => {
    await testenv.up("v15");
    // Fake system browser: the app inherits PATH from tauri-driver, so restart after setting it.
    opener = installOpenerStub();
    process.env.PATH = `${opener.binDir}:${process.env.PATH ?? ""}`;
    await app.restart();
    await finishWizard();
    await addFirstServer(64738);
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
      await $("//*[@aria-label='Image to send' or @alt='Image to send']").waitForExist({ timeout: 10_000 });
      await sendText("**bold** words");
      const got = await listener.run(`expect message --from ${USER} --contains "<img"`);
      expect(got).toMatch(/<(b|strong)>bold<\/(b|strong)>/);
      expect(got).toMatch(/<img src="data:image\/(png|jpeg);base64,/);
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
      const preview = $("//*[@aria-label='Image to send' or @alt='Image to send']");
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
});
