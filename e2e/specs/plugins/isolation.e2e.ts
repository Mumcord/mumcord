// Spec 09 (plugin platform 1.1): isolation of a plugin's web page and the keyboard around it,
// with `test-surface` from the third-party fixture repository and the HTTPS fixture as the only
// network target (declared.plugins.test is declared, undeclared.plugins.test is not; e2e/run.sh
// maps both to 127.0.0.1).
//
//   AC-9.2  in the stage (and the side panel): `fetch` to the declared origin works; to an
//           undeclared origin, to http://, XHR, WebRTC, image beacon, sendBeacon, prefetch,
//           preconnect, DNS prefetch, preload, CSS, EventSource, WebSocket, <a ping>, download,
//           workers, remote script, window.open and navigation fail. The page's own status is
//           not enough for the ones it cannot observe, so the HTTPS fixture's log decides: after
//           all probes it saw nothing but the declared fetch (spec 09 §13.1: a TCP or TLS
//           connection counts as a leak).
//   AC-9.3  the page cannot reach Tauri IPC, the parent's or another frame's document, storage,
//           cookies, popups, modals, the clipboard, camera or microphone.
//   AC-9.5  Tab enters the frame, Esc and F6 inside it return focus to Mumcord in the stage, the
//           side panel and the dialog; in the dialog Esc also closes it.
//
// Not automated here:
//   - AC-9.2 "Diagnostics shows the blocked count": the frame's blocked attempts never reach
//     Mumcord (the web view drops them under the page's CSP), so only `net.http` attempts are
//     counted (FR-9.27 gap, reported with this spec).
//   - Windows and macOS (WebView2, child web views) and signed builds: manual checklist AC-9.40.
import { $, browser, expect } from "@wdio/globals";
import { pluginenv } from "../../helpers/index.ts";
import * as surface from "../../helpers/test-surface.ts";

/** The probes "Run all probes" runs (web/probe.js without the navigating ones). */
const NETWORK = [
  "fetch-undeclared",
  "fetch-http",
  "xhr",
  "webrtc",
  "img-beacon",
  "preload",
  "css-import",
  "eventsource",
  "websocket",
  "workers",
  "remote-script",
] as const;
/** Probes the page cannot observe: `sent` is fine as long as the fixture saw nothing. */
const UNOBSERVABLE = [
  "send-beacon",
  "prefetch",
  "preconnect",
  "dns-prefetch",
  "css-url",
  "a-ping",
  "download",
] as const;
const ESCAPES = [
  "ipc",
  "parent-dom",
  "other-frames",
  "storage",
  "cookies",
  "window-open",
  "modals",
  "clipboard",
  "media",
  "csp-eval",
] as const;
const ALLOWED = ["fetch-declared", "csp-wasm"] as const;
const ALL = [...ALLOWED, ...NETWORK, ...UNOBSERVABLE, ...ESCAPES];
/** Probes that navigate the frame; each is sent on its own. */
const NAVIGATING = ["navigation", "top-navigation", "meta-refresh", "form"] as const;

const DECLARED_HOST = "declared.plugins.test";

const stageFrame = () => $(".plugin-stage iframe.plugin-webview__frame");
const sideFrame = () => $(".plugin-sidepanel iframe.plugin-webview__frame");
const dialog = () => $("div.plugin-web-dialog[role='dialog']");
const dialogFrame = () => dialog().$("iframe.plugin-webview__frame");

/** What reached the fixture apart from the declared fetch (`GET /` on the declared host). */
function leaks(): string[] {
  return pluginenv
    .httpsLog()
    .filter((hit) => !(hit.host === DECLARED_HOST && (hit.kind === "tls" || hit.path === "/")))
    .map((hit) => `${hit.kind} ${hit.host || "(no name)"}${hit.path}`);
}

/** Sends a command to the page the way its host does (the page accepts only its parent). */
async function postToFrame(selector: string, message: string): Promise<void> {
  await browser.execute(
    (sel: string, text: string) => {
      const frame = document.querySelector<HTMLIFrameElement>(sel);
      if (frame?.contentWindow == null) throw new Error(`no frame ${sel}`);
      frame.contentWindow.postMessage(text, "*");
    },
    selector,
    message,
  );
}

/** What has focus in the app window: `frame` (inside a plugin page), `wrapper` (the frame's
 * focusable wrapper), `body` or the class of the focused element. */
async function focused(): Promise<string> {
  return browser.execute(() => {
    const active = document.activeElement;
    if (active === null || active === document.body) return "body";
    if (active instanceof HTMLIFrameElement) return "frame";
    if (active.classList.contains("plugin-webview")) return "wrapper";
    return `${active.tagName.toLowerCase()}.${active.className}`;
  });
}

/** Moves focus to the wrapper in front of `frame`, then Tab moves it into the page. */
async function tabInto(frame: ReturnType<typeof $>): Promise<void> {
  await browser.execute((el) => {
    el.closest<HTMLElement>(".plugin-webview")?.focus();
  }, frame);
  await browser.waitUntil(async () => (await focused()) === "wrapper", {
    timeout: 5_000,
    timeoutMsg: "the frame's wrapper did not take focus",
  });
  await browser.keys("Tab");
  await browser.waitUntil(async () => (await focused()) === "frame", {
    timeout: 5_000,
    timeoutMsg: "Tab did not move focus into the plugin page",
  });
}

/** Presses `key` inside the page and expects focus back on Mumcord's side of the frame. */
async function leaveWith(key: "Escape" | "F6"): Promise<void> {
  await browser.keys(key);
  let now = "";
  await browser.waitUntil(
    async () => {
      now = await focused();
      return now === "wrapper";
    },
    { timeout: 5_000, timeoutMsg: `${key} left focus at ${now}` },
  );
}

describe("plugins 1.1: web frame isolation and keyboard (AC-9.2, AC-9.3, AC-9.5)", function () {
  this.timeout(20 * 60_000);

  before(async function () {
    this.timeout(15 * 60_000);
    await surface.setUp();
    await surface.panelButton("Start stage");
    await stageFrame().waitForExist({ timeout: 15_000, timeoutMsg: "the stage frame did not open" });
    await surface.waitLine(/^ready: stage$/);
    await surface.panelButton("Open side panel");
    await sideFrame().waitForExist({ timeout: 15_000, timeoutMsg: "the side panel did not open" });
    await surface.waitLine(/^ready: side$/);
  });

  after(async () => {
    await pluginenv.stopPluginFixtures();
  });

  describe("probes in the stage and the side panel", () => {
    let stage: Record<string, string> = {};
    let side: Record<string, string> = {};

    before(async function () {
      this.timeout(6 * 60_000);
      pluginenv.clearHttpsLog();
      await surface.panelButton("Run all probes");
      stage = await surface.waitResults("stage", ALL);
      side = await surface.waitResults("side", ALL);
    });

    it("AC-9.2: fetch to the declared origin works, WebAssembly compiles under the CSP", () => {
      for (const results of [stage, side]) {
        for (const probe of ALLOWED) expect(`${probe}: ${results[probe]}`).toBe(`${probe}: ok`);
      }
    });

    it("AC-9.2: every way to an undeclared origin fails and nothing reaches it", () => {
      for (const results of [stage, side]) {
        for (const probe of NETWORK) expect(`${probe}: ${results[probe]}`).toBe(`${probe}: blocked`);
        for (const probe of UNOBSERVABLE) {
          expect(["blocked", "sent"]).toContain(results[probe]);
        }
      }
      expect(leaks()).toEqual([]);
    });

    it("AC-9.3: no IPC, no parent or other frame, no storage, cookies, popups, modals, clipboard or devices", () => {
      for (const results of [stage, side]) {
        for (const probe of ESCAPES) expect(`${probe}: ${results[probe]}`).toBe(`${probe}: blocked`);
      }
      // A modal that showed would have held the page; the app window answers right away.
      expect(leaks()).toEqual([]);
    });
  });

  it("AC-9.2: navigation, top navigation, meta refresh and form submission go nowhere", async function () {
    this.timeout(3 * 60_000);
    pluginenv.clearHttpsLog();
    for (const probe of NAVIGATING) {
      await postToFrame(".plugin-sidepanel iframe.plugin-webview__frame", `run:side:${probe}`);
      // A navigation that went through replaces the page, so it never reports; the host then
      // removes the frame (PluginFrame: a second load is a navigation).
      const line = await surface.waitLine(new RegExp(`^side/${probe}: (blocked|error)$`), 20_000);
      expect(line).toBe(`side/${probe}: blocked`);
      await expect(sideFrame()).toBeExisting();
    }
    // The main window never left the app either.
    expect(await browser.execute(() => location.protocol)).not.toBe("https:");
    expect(leaks()).toEqual([]);
  });

  it("AC-9.5: in the stage, Tab enters the page and Esc or F6 return focus to Mumcord", async () => {
    await tabInto(stageFrame());
    await leaveWith("Escape");
    await tabInto(stageFrame());
    await leaveWith("F6");
    await expect(stageFrame()).toBeExisting();
  });

  it("AC-9.5: in the side panel, Tab enters the page and Esc or F6 return focus to Mumcord", async () => {
    await tabInto(sideFrame());
    await leaveWith("Escape");
    await tabInto(sideFrame());
    await leaveWith("F6");
    await expect(sideFrame()).toBeExisting();
  });

  it("AC-9.5: in the dialog, F6 returns focus and Esc closes it", async () => {
    await surface.panelButton("Open dialog");
    await dialogFrame().waitForExist({ timeout: 15_000, timeoutMsg: "the web dialog did not open" });
    await surface.waitLine(/^ready: dialog$/);
    await tabInto(dialogFrame());
    await leaveWith("F6");
    await expect(dialog()).toBeDisplayed();
    await tabInto(dialogFrame());
    await browser.keys("Escape");
    await dialog().waitForExist({ reverse: true, timeout: 5_000, timeoutMsg: "Esc did not close the dialog" });
    // Focus is back in Mumcord, not lost with the removed frame.
    expect(await focused()).not.toBe("body");
    expect(await focused()).not.toBe("frame");
  });
});
