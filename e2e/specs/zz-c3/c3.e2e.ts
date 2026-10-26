import { browser, $ } from "@wdio/globals";
import { finishWizard, clickButton, trustIfAsked } from "../../helpers/chat-ui.ts";

// TEMPORARY QA spec for C3 (AC-2.17..2.19); deleted after the run.
const TALK = "/home/tvorwachs/code/mumble/e2e/artifacts/c3/talk"; void TALK;
const out = (k: string, v: unknown) => console.log(`C3|${k}|${JSON.stringify(v)}`);
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function installMonitor() {
  await browser.execute(() => {
    const w = window as any;
    w.__c3 = { gaps: [] as number[][], syncFirst: null, syncLast: null, syncSeen: 0 };
    let last = performance.now();
    const loop = () => {
      const n = performance.now();
      if (n - last > 50) w.__c3.gaps.push([Math.round(n), Math.round(n - last)]);
      last = n;
      requestAnimationFrame(loop);
    };
    requestAnimationFrame(loop);
    new MutationObserver(() => {
      const has = [...document.querySelectorAll('[role="status"]')].some((e) =>
        (e.textContent ?? "").includes("Syncing big"),
      );
      if (has) {
        w.__c3.syncSeen++;
        w.__c3.syncFirst ??= Math.round(performance.now());
        w.__c3.syncLast = Math.round(performance.now());
      }
    }).observe(document.body, { subtree: true, childList: true, characterData: true });
  });
}
async function readMonitor(reset = true) {
  return browser.execute((rst: boolean) => {
    const w = window as any;
    const c = w.__c3;
    const gaps: number[][] = c.gaps;
    const r = {
      now: Math.round(performance.now()),
      maxGap: gaps.reduce((m, g) => Math.max(m, g[1]), 0),
      top: [...gaps].sort((a, b) => b[1] - a[1]).slice(0, 6),
      n: gaps.length,
      syncSeen: c.syncSeen, syncFirst: c.syncFirst, syncLast: c.syncLast,
    };
    if (rst) { c.gaps = []; c.syncSeen = 0; c.syncFirst = null; c.syncLast = null; }
    return r;
  }, reset);
}

describe("C3", () => {
  it("idle baseline", async () => {
    await finishWizard("C3QA");
    const input = $("//label[.='Server address or mumble:// link']/following::input[1]");
    await input.waitForDisplayed({ timeout: 30_000 });
    await installMonitor();
    await sleep(3000);
    out("idleBaseline", await readMonitor());
  });

  it("AC-2.17 connect to scale server", async () => {
    const input = $("//label[.='Server address or mumble:// link']/following::input[1]");
    await input.setValue("127.0.0.1:64850");
    await readMonitor();
    await clickButton("Add and connect");
    await trustIfAsked();
    const t0 = Date.now();
    await $("button.userpanel__toggle--mic").waitForEnabled({ timeout: 60_000 });
    out("connectedAfterMs", Date.now() - t0);
    await sleep(8000);
    out("connectMonitor", await readMonitor());
    out("dom", await browser.execute(() => ({
      channels: document.querySelectorAll(".tree-channel").length,
      people: document.querySelectorAll(".tree-person").length,
      html: document.querySelector(".channel-tree")?.outerHTML.slice(0, 1500),
    })));
  });
});
