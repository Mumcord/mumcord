import { browser, $, $$ } from "@wdio/globals";
import { testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  composer,
  finishWizard,
  joinChannel,
  ScriptBot,
  selectChannel,
  sendText,
  waitMessage,
  sleep,
} from "../../helpers/chat-ui.ts";

// TEMPORARY QA spec for C6; deleted after the run.
const SHOTS = "/tmp/claude-1000/-home-tvorwachs-code-mumble/c058c4ad-51cc-4d30-ac13-c419b31798cb/scratchpad/c6";
const out = (k: string, v: unknown) => console.log(`C6|${k}|${JSON.stringify(v)}`);
const shot = (n: string) => browser.saveScreenshot(`${SHOTS}/${n}.png`);

describe("c6a", () => {
  let botA: ScriptBot;
  const port = () => testenv.ports.v15;

  before(async () => {
    await testenv.up("v15");
    await finishWizard();
    await addFirstServer(port());
    await joinChannel("Lobby");
    botA = new ScriptBot({ name: "BotA", port: port() });
    await botA.connected();
    await botA.run("join Lobby");
  });
  after(() => botA.stop());

  it("2.41 reply", async () => {
    await botA.run(`say --channel Lobby "original message one"`);
    await botA.run(`say --channel Lobby "<script>alert(1)</script>evil <b>bold</b>"`);
    await waitMessage("original message one");
    const li = $("//li[contains(.,'original message one')]");
    await li.moveTo();
    const btn = li.$(".//button[starts-with(@aria-label,'Reply to BotA')]");
    await btn.waitForClickable();
    await btn.click();
    await $(".chat-composer__reply").waitForDisplayed();
    out("2.41.preview", await $(".chat-composer__reply").getText());
    await shot("2.41-preview");
    await sendText("my reply here");
    await waitMessage("my reply here");
    const hdr = $("//li[contains(.,'my reply here')]//*[contains(@class,'chat-reply--jump')]");
    await hdr.waitForDisplayed();
    out("2.41.header", await hdr.getText());
    // push original out of view
    for (let i = 0; i < 25; i++) await botA.run(`say --channel Lobby "filler line ${i}"`);
    await sleep(500);
    await hdr.scrollIntoView();
    await hdr.click();
    await sleep(500);
    const hl = await browser.execute(() =>
      Array.from(document.querySelectorAll(".chat-list__items > li"))
        .filter((l) => /highlight|flash|target/.test(l.className))
        .map((l) => (l as HTMLElement).innerText.slice(0, 40)),
    );
    out("2.41.highlighted", hl);
    await shot("2.41-jump");
    // bot receives readable quote
    const got = await botA.run(`expect message --contains "my reply here"`);
    out("2.41.botGot", got);
    // script/html quote shown as plain text
    const li2 = $("//li[contains(.,'evil')]");
    await li2.scrollIntoView();
    await li2.moveTo();
    await li2.$(".//button[starts-with(@aria-label,'Reply to BotA')]").click();
    await sendText("reply to evil");
    await waitMessage("reply to evil");
    const hdr2 = await $("//li[contains(.,'reply to evil')]//*[contains(@class,'chat-reply')]").getHTML();
    out("2.41.evilHeader", hdr2);
    await shot("2.41-evil");
  });

  it("2.42 avatar alignment / 2.43 sticky", async () => {
    const lines = Array.from({ length: 40 }, (_, i) => `longline ${i}`).join("<br>");
    await botA.run(`say --channel Lobby "${lines}"`);
    await waitMessage("longline 39");
    const li = $("//li[contains(.,'longline 39')]");
    await li.scrollIntoView({ block: "end" });
    await sleep(500);
    const geo = await browser.execute(() => {
      const li = Array.from(document.querySelectorAll(".chat-list__items > li")).find((l) =>
        (l as HTMLElement).innerText.includes("longline 39"),
      ) as HTMLElement;
      const r = li.getBoundingClientRect();
      const img = li.querySelector("img, .avatar, [class*=avatar]") as HTMLElement | null;
      const name = li.querySelector(".chat-message__sender") as HTMLElement | null;
      return {
        li: [r.top, r.height],
        avatar: img ? [img.getBoundingClientRect().top, img.getBoundingClientRect().height] : null,
        name: name ? [name.getBoundingClientRect().top, name.getBoundingClientRect().height] : null,
      };
    });
    out("2.42.geometry", geo);
    await shot("2.42");
    // scroll to middle of the long message
    const mid = await browser.execute(() => {
      const list = document.querySelector(".chat-list") as HTMLElement;
      const li = Array.from(document.querySelectorAll(".chat-list__items > li")).find((l) =>
        (l as HTMLElement).innerText.includes("longline 39"),
      ) as HTMLElement;
      const scroller = [list, list?.parentElement].find((e) => e && e.scrollHeight > e.clientHeight) as HTMLElement;
      const lr = li.getBoundingClientRect();
      const sr = scroller.getBoundingClientRect();
      scroller.scrollTop += lr.top - sr.top + lr.height / 2 - 100;
      return scroller.className;
    });
    await sleep(600);
    out("2.43.scroller", mid);
    const sticky = await browser.execute(() =>
      Array.from(document.querySelectorAll("[class*=sticky]")).map((e) => [e.className, (e as HTMLElement).innerText.slice(0, 40), e.getBoundingClientRect().top, e.getBoundingClientRect().width]),
    );
    out("2.43.sticky", sticky);
    await shot("2.43-mid");
    await browser.execute(() => {
      const list = document.querySelector(".chat-list") as HTMLElement;
      const s = [list, list?.parentElement].find((e) => e && e.scrollHeight > e.clientHeight) as HTMLElement;
      s.scrollTop = s.scrollHeight;
    });
    await sleep(600);
    const sticky2 = await browser.execute(() =>
      Array.from(document.querySelectorAll("[class*=sticky]")).map((e) => [e.className, (e as HTMLElement).innerText.slice(0, 40)]),
    );
    out("2.43.stickyAfter", sticky2);
  });

  it("2.49 quote shortcut", async () => {
    for (const t of ["> hello", ">_>", ">hello"]) {
      await sendText(t);
      await sleep(600);
    }
    await shot("2.49");
    const html = await browser.execute(() =>
      Array.from(document.querySelectorAll(".chat-list__items > li")).slice(-3).map((l) => l.querySelector(".chat-message__body")?.innerHTML ?? l.innerHTML),
    );
    out("2.49.html", html);
  });

  it("2.52 link menu", async () => {
    await botA.run(`say --channel Lobby "see <a href='https://example.org/path?x=1'>this link</a> ok"`);
    await waitMessage("this link");
    const a = $("//li[contains(.,'this link')]//a[contains(.,'this link')]");
    await a.scrollIntoView();
    await a.click({ button: "right" });
    await sleep(400);
    const items = await $$("[role=menuitem]").map((e) => e.getText());
    out("2.52.menu", items);
    await shot("2.52");
    await browser.keys("Escape");
    // keyboard
    await browser.execute(() => {
      const els = Array.from(document.querySelectorAll("a")).filter((e) => e.textContent === "this link");
      (els[els.length - 1] as HTMLElement).focus();
    });
    await browser.keys("ContextMenu");
    await sleep(400);
    out("2.52.menuKb", await $$("[role=menuitem]").map((e) => e.getText()));
    const copy = $("//*[@role='menuitem' and contains(.,'Copy link')]");
    if (await copy.isExisting()) {
      await copy.click();
      await sleep(300);
      out("2.52.toast", await browser.execute(() => document.body.innerText.includes("Link copied")));
    }
  });

  it("2.39 listeners inline", async () => {
    const bl = new ScriptBot({ name: "BotL", port: port() });
    await bl.connected();
    await bl.run("listen Lobby");
    await sleep(1500);
    const rows = await browser.execute(() =>
      Array.from(document.querySelectorAll("[class*=tree-person]")).map((e) => (e as HTMLElement).className + "|" + (e as HTMLElement).innerText.replace(/\n/g, " ").slice(0, 40)),
    );
    out("2.39.rows", rows);
    out("2.39.ear", await browser.execute(() => Array.from(document.querySelectorAll("[aria-label*='Listening'],[title*='Listening'],[data-tooltip*='Listening']")).map((e) => e.outerHTML.slice(0, 200))));
    out("2.39.heading", await browser.execute(() => document.body.innerText.includes("Listening\n")));
    await shot("2.39");
    await bl.run("unlisten Lobby");
    await sleep(1500);
    out("2.39.goneAfter", await browser.execute(() => !document.body.innerText.includes("BotL")));
    bl.stop();
  });

  it("2.46 apps menu", async () => {
    await selectChannel("Lobby");
    const apps = $("button[aria-haspopup='menu'][aria-label^='Apps']");
    out("2.46.appsExists", await apps.isExisting());
    await apps.click();
    await sleep(300);
    out("2.46.items", await $$("[role=menuitem]").map((e) => e.getText()));
    await shot("2.46");
    await browser.keys("Escape");
    await selectChannel("Raid");
    out("2.46.otherChannelApps", await $("button[aria-haspopup='menu'][aria-label^='Apps']").isExisting());
  });

  it("2.60 tooltips", async () => {
    const targets = await $$("[data-tooltip]");
    out("2.60.count", targets.length);
    const first = targets[0];
    await first.moveTo();
    await sleep(900);
    const tip = await browser.execute(() =>
      Array.from(document.querySelectorAll("[role=tooltip]")).map((e) => [e.className, (e as HTMLElement).innerText, JSON.stringify(e.getBoundingClientRect())]),
    );
    out("2.60.tip", tip);
    await shot("2.60");
  });

  it("2.55 send feedback", async () => {
    await selectChannel("Lobby");
    await testenv.stop("v15");
    await sleep(500);
    await composer().setValue("offline msg");
    await browser.keys("Enter");
    await sleep(2500);
    out("2.55.cleared", await composer().getValue());
    const li = $("//li[contains(.,'offline msg')]");
    out("2.55.li", await li.isExisting() ? (await li.getHTML()).slice(0, 700) : null);
    await shot("2.55-offline");
    await testenv.start("v15");
  });
});
