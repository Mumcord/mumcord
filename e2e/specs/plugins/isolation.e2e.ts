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
/** The stage's representative set (see "probes in the stage"). */
const STAGE_PROBES = ["fetch-declared", "fetch-undeclared", "ipc", "parent-dom"] as const;
/** Probes that navigate the frame (a download link to another origin does on WebKitGTK); each is
 * sent on its own. */
const NAVIGATING = ["navigation", "top-navigation", "meta-refresh", "form", "download"] as const;

const DECLARED_HOST = "declared.plugins.test";

const STAGE_FRAME = ".plugin-stage iframe.plugin-webview__frame";
const SIDE_FRAME = ".plugin-sidepanel iframe.plugin-webview__frame";
const DIALOG = "div.plugin-web-dialog[role='dialog']";
const DIALOG_FRAME = `${DIALOG} iframe.plugin-webview__frame`;
const stageFrame = () => $(STAGE_FRAME);
const sideFrame = () => $(SIDE_FRAME);
const dialog = () => $(DIALOG);
const dialogFrame = () => $(DIALOG_FRAME);

/** How often the panel log says the side page loaded. */
async function sideReadyCount(): Promise<number> {
  return (await surface.panelLines()).filter((l) => l === "ready: side").length;
}

/** Closes the side panel and opens it again, so it shows a fresh page. */
async function reopenSide(): Promise<void> {
  const before = await sideReadyCount();
  const close = $(".plugin-sidepanel__head .plugin-sidepanel__tool:last-of-type");
  if (await close.isExisting()) {
    await close.click();
    await $(".plugin-sidepanel").waitForExist({ reverse: true, timeout: 10_000 });
  }
  await surface.panelButton("Open side panel");
  await sideFrame().waitForExist({ timeout: 15_000, timeoutMsg: "the side panel did not open" });
  await browser.waitUntil(async () => (await sideReadyCount()) > before, {
    timeout: 15_000,
    timeoutMsg: "the side page did not load again",
  });
}

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

/** Moves focus to the wrapper in front of the frame at `frameSelector`, then Tab moves it into the page. */
async function tabInto(frameSelector: string): Promise<void> {
  await browser.execute((sel: string) => {
    document.querySelector(sel)?.closest<HTMLElement>(".plugin-webview")?.focus();
  }, frameSelector);
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

/** Starts the stage unless its frame is up; waits for its page. */
async function ensureStage(): Promise<void> {
  if (await stageFrame().isExisting()) return;
  const before = (await surface.panelLines()).filter((l) => l === "ready: stage").length;
  await surface.panelButton("Start stage");
  await stageFrame().waitForExist({ timeout: 15_000, timeoutMsg: "the stage frame did not open" });
  await browser.waitUntil(
    async () =>
      (await surface.panelLines()).filter((l) => l === "ready: stage").length > before,
    { timeout: 15_000, timeoutMsg: "the stage page did not load" },
  );
}

/** Opens the side panel unless its frame is up; waits for its page. */
async function ensureSide(): Promise<void> {
  if (await sideFrame().isExisting()) return;
  await reopenSide();
}

/** The plugin's dialog (tree or web view) as the host would restore it in a new snapshot. */
async function dialogInSnapshot(): Promise<string[]> {
  const snapshot = (await surface.invokeOk("plugins_ui_trees")) as {
    plugin: string;
    change: { type: string; place?: string };
  }[];
  return snapshot
    .filter(({ plugin }) => plugin === surface.ID)
    .map(({ change }) => change)
    .filter((c) => c.type === "openDialog" || (c.type === "webviewShow" && c.place === "dialog"))
    .map((c) => c.type);
}

/** Closes the plugin's dialog with its ✕ if a test left it open. */
async function closeDialogIfOpen(): Promise<void> {
  if (!(await dialog().isExisting())) return;
  await $(`${DIALOG} button.plugin-web-dialog__close`).click();
  await dialog().waitForExist({ reverse: true, timeout: 5_000, timeoutMsg: "✕ did not close the dialog" });
}

describe("plugins 1.1: web frame isolation and keyboard (AC-9.2, AC-9.3, AC-9.5)", function () {
  this.timeout(20 * 60_000);

  before(async function () {
    this.timeout(15 * 60_000);
    await surface.setUp();
  });

  after(async () => {
    await pluginenv.stopPluginFixtures();
  });

  // Every group opens the frames it needs and leaves no dialog behind, so none depends on what an
  // earlier group did or on the order mocha runs them in.
  afterEach(closeDialogIfOpen);

  // The full probe set runs in the side panel. In the full spec flow the stage page stalls on
  // its IndexedDB probe (and both frames stall on WebAssembly.compile when they run at once), so
  // the stage gets a representative set, one probe per call; it renders the same PluginFrame with
  // the same sandbox, CSP and Permissions-Policy as the side panel (none of them depends on the
  // place). See issue #560.
  describe("probes in the side panel", () => {
    let side: Record<string, string> = {};

    before(async function () {
      this.timeout(6 * 60_000);
      await ensureSide();
      pluginenv.clearHttpsLog();
      await postToFrame(SIDE_FRAME, "run:side:all");
      side = await surface.waitResults("side", ALL);
    });

    it("AC-9.2: fetch to the declared origin works, WebAssembly compiles under the CSP", () => {
      for (const probe of ALLOWED) expect(`${probe}: ${side[probe]}`).toBe(`${probe}: ok`);
    });

    it("AC-9.2: every way to an undeclared origin fails and nothing reaches it", () => {
      for (const probe of NETWORK) expect(`${probe}: ${side[probe]}`).toBe(`${probe}: blocked`);
      for (const probe of UNOBSERVABLE) expect(["blocked", "sent"]).toContain(side[probe]);
      expect(leaks()).toEqual([]);
    });

    it("AC-9.3: no IPC, no parent or other frame, no storage, cookies, popups, modals, clipboard or devices", () => {
      for (const probe of ESCAPES) expect(`${probe}: ${side[probe]}`).toBe(`${probe}: blocked`);
      // A modal that showed would have held the page; the app window answers right away.
      expect(leaks()).toEqual([]);
    });
  });

  describe("probes in the stage", () => {
    before(ensureStage);

    it("AC-9.2, AC-9.3: the stage fetches its declared origin and nothing else, and reaches no IPC or parent", async () => {
      pluginenv.clearHttpsLog();
      const results: Record<string, string> = {};
      for (const probe of STAGE_PROBES) {
        await postToFrame(STAGE_FRAME, `run:stage:${probe}`);
        results[probe] = (await surface.waitResults("stage", [probe]))[probe] ?? "";
      }
      expect(results).toEqual({
        "fetch-declared": "ok",
        "fetch-undeclared": "blocked",
        ipc: "blocked",
        "parent-dom": "blocked",
      });
      expect(leaks()).toEqual([]);
    });
  });

  describe("navigation in the side panel", () => {
    before(ensureSide);

    it("AC-9.2: navigation, top navigation, meta refresh, form submission and download go nowhere", async function () {
      this.timeout(5 * 60_000);
      pluginenv.clearHttpsLog();
      for (const probe of NAVIGATING) {
        await postToFrame(SIDE_FRAME, `run:side:${probe}`);
        // The host removes a frame whose page loads a second time (PluginFrame: that is a
        // navigation), so a probe either reports "blocked" or its frame disappears. Either way the
        // HTTPS fixture decides whether anything left the frame.
        let outcome = "";
        await browser.waitUntil(
          async () => {
            const line = (await surface.panelLines()).find((l) => l.startsWith(`side/${probe}: `));
            if (line !== undefined && !line.endsWith(": running")) outcome = line;
            else if (!(await sideFrame().isExisting())) outcome = "removed";
            return outcome !== "";
          },
          { timeout: 20_000, interval: 250, timeoutMsg: `side/${probe} neither reported nor closed` },
        );
        // A download the page cannot observe reports "sent"; the HTTPS fixture decides.
        const sent = probe === "download" ? [`side/${probe}: sent`] : [];
        expect([`side/${probe}: blocked`, "removed", ...sent]).toContain(outcome);
        if (outcome === "removed") await reopenSide();
      }
      // The main window never left the app either.
      expect(await browser.execute(() => location.protocol)).not.toBe("https:");
      expect(leaks()).toEqual([]);
    });
  });

  describe("keyboard", () => {
    it("AC-9.5: in the stage, Tab enters the page and Esc or F6 return focus to Mumcord", async () => {
      await ensureStage();
      await tabInto(STAGE_FRAME);
      await leaveWith("Escape");
      await tabInto(STAGE_FRAME);
      await leaveWith("F6");
      await expect(stageFrame()).toBeExisting();
    });

    it("AC-9.5: in the side panel, Tab enters the page and Esc or F6 return focus to Mumcord", async () => {
      await ensureSide();
      await tabInto(SIDE_FRAME);
      await leaveWith("Escape");
      await tabInto(SIDE_FRAME);
      await leaveWith("F6");
      await expect(sideFrame()).toBeExisting();
    });

    it("AC-9.5: in the dialog, F6 returns focus and Esc closes it", async () => {
      await surface.panelButton("Open dialog");
      await dialogFrame().waitForExist({ timeout: 15_000, timeoutMsg: "the web dialog did not open" });
      await surface.waitLine(/^ready: dialog$/);
      await tabInto(DIALOG_FRAME);
      await leaveWith("F6");
      await expect(dialog()).toBeDisplayed();
      await tabInto(DIALOG_FRAME);
      await browser.keys("Escape");
      await dialog().waitForExist({ reverse: true, timeout: 5_000, timeoutMsg: "Esc did not close the dialog" });
      // Focus is back on the panel button that opened it, not lost with the removed frame.
      await expect($("//*[contains(@class,'pstrip-body')]//button[normalize-space()='Open dialog']")).toBeFocused();
      // The dialog ended on the host side too: a later snapshot (any plugin change reloads it)
      // must not bring its tree back as a modal nobody opened.
      expect(await dialogInSnapshot()).toEqual([]);
    });
  });
});
