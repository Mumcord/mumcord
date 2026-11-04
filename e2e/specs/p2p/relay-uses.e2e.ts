// Spec 07, relay "allowed uses" (AC-7.1 – AC-7.5) in the running client, against the mumpeer test
// environment (helpers/relayenv.ts). Client A is the one under test; B is the other Mumcord user in
// the same channel (sharer, image sender).
//
//   AC-7.1  default relay: the server info panel says "Relay allows: Images · Files · Screen sharing"
//           and /v1/status lists the three uses with a broadcast limit.
//   AC-7.2  files off: the info row drops "Files". (The paperclip and the offer card with their
//           reason come with the file sharing UI, which has its own specs in specs/files.)
//   AC-7.3  screen sharing off: A in "Relay only" cannot share and cannot watch B, both with the
//           reason text; with a direct path allowed (default mode) watching B is still possible.
//   AC-7.4  images off: the info row drops "Images"; a full-size image that cannot be fetched says
//           that the relay doesn't allow images (the generic text when it does).
//   AC-7.5  an old relay without the allowed uses behaves as "all allowed".
//
// Not automated: AC-7.3's "Watch unavailable with netns.sh nodirect" and AC-7.4's "the full image
// arrives over plugin data and the relay log shows no session" need the netns.sh namespaces (sudo)
// and the relay's log; the reason texts and the row are what they decide, and those are checked here.
//
// Skips itself without a mumpeer checkout; the relay bot must be a mumbot with the `peer` module
// (MUMBOT_IMAGE, see .github/workflows/e2e.yml).
import { browser, $, expect } from "@wdio/globals";
import { apps, relayenv } from "../../helpers/index.ts";
import { composer, pasteImage, startScreenShare } from "../../helpers/chat-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { addAndConnect, completeWizard, personRow } from "../../helpers/voice-ui.ts";

const oldImage = process.env.MUMCORD_E2E_OLD_MUMPEER_IMAGE;
const oldBotImage = process.env.MUMCORD_E2E_OLD_MUMBOT_IMAGE;

const SCREEN_REASON = "This server's relay doesn't allow screen sharing.";
const ALL_USES = "Images · Files · Screen sharing";

type Mode = "directRelay" | "relayOnly";

/** The value next to `label` in the open server info panel. */
function infoValue(label: string) {
  return $(`//dt[normalize-space()='${label}']/following-sibling::dd[1]`);
}

/** Opens the server info panel, waits until "Relay allows" says `expected`, and closes it again. */
async function expectRelayAllows(expected: string): Promise<void> {
  await $("button[aria-label^='Server options']").click();
  await $("//*[@role='menuitem'][normalize-space()='Server info']").click();
  const row = infoValue("Relay allows");
  let seen = "";
  try {
    await browser.waitUntil(
      async () => {
        seen = (await row.isExisting()) ? (await row.getText()).trim() : "";
        return seen === expected;
      },
      { timeout: 60_000, interval: 1000 },
    );
  } catch {
    throw new Error(`Relay allows: want "${expected}", got "${seen}"`);
  } finally {
    await browser.keys("Escape");
  }
}

/** Settings → Privacy & Connections: picks the peer-to-peer mode of the running client. */
async function setMode(mode: Mode): Promise<void> {
  await openSettingsGroup("Privacy & Connections");
  const radio = $(`fieldset.p2p-modes input[type='radio'][value='${mode}']`);
  await radio.waitForExist();
  await radio.parentElement().click();
  await browser.waitUntil(async () => radio.isSelected(), { timeoutMsg: `${mode} was not selected` });
  await browser.keys("Escape");
  await $("fieldset.p2p-modes").waitForExist({ reverse: true });
}

/** Joins the shown server's channel when its read-only notice offers it. */
async function joinVoice(): Promise<void> {
  const join = $(".userpanel-readonly__join");
  if (!(await join.isDisplayed())) return;
  await join.click();
  await $(".userpanel-readonly").waitForExist({ reverse: true, timeout: 15_000 });
}

/**
 * The reason on "Share screen" in the composer's Apps menu, `null` when sharing is possible.
 * Focusing the menu item reads the engine's block again, so this waits for `expected`.
 */
async function expectShareReason(expected: string | null): Promise<void> {
  const button = $("button.chat-apps__button");
  await button.waitForClickable({ timeout: 20_000 });
  await button.click();
  const item = $(
    "//div[contains(@class,'chat-apps__menu')]//*[@role='menuitem'][starts-with(normalize-space(),'Share screen')]",
  );
  await item.waitForDisplayed();
  const read = async (): Promise<string | null> => {
    const reason = item.$(".chat-apps__reason");
    return (await reason.isExisting()) ? (await reason.getText()).trim() : null;
  };
  try {
    await browser.waitUntil(async () => (await read()) === expected, { timeout: 20_000 });
    expect(await item.getAttribute("aria-disabled")).toBe(expected === null ? null : "true");
  } catch (error) {
    throw new Error(`share reason: want ${JSON.stringify(expected)}, got ${JSON.stringify(await read())} (${String(error)})`);
  } finally {
    await browser.keys("Escape");
  }
}

/** B joins the server of the running relay environment as `name` (fresh profile). */
async function startB(name: string, port: number): Promise<void> {
  await apps.startB();
  await apps.onB(async () => {
    await completeWizard(name);
    await addAndConnect(`localhost:${String(port)}`);
    await joinVoice();
  });
}

/** B starts sharing its screen (Xvfb: the whole X screen, no picker). */
async function bStartsSharing(): Promise<void> {
  await apps.onB(startScreenShare);
}

/** B stops its share. */
async function bStopsSharing(): Promise<void> {
  await apps.onB(async () => {
    await $(".sharer-bar button[aria-label='Stop sharing']").click();
    await $("//div[contains(@class,'sharer-bar')]").waitForExist({ reverse: true, timeout: 20_000 });
  });
}

/** The "Watch" button of B's share banner in A's channel. */
function watchButton() {
  return $("//div[contains(@class,'activity-banner')][contains(.,'is sharing their screen')]//button[normalize-space()='Watch']");
}

/**
 * A waits for B's banner and checks the button: enabled when `reason` is null, else
 * aria-disabled with that text as tooltip and screen reader description.
 */
async function expectWatch(reason: string | null): Promise<void> {
  const button = watchButton();
  await button.waitForDisplayed({ timeout: 30_000, timeoutMsg: "A never saw B's share" });
  try {
    await browser.waitUntil(
      async () => ((await button.getAttribute("data-tooltip")) ?? null) === reason,
      { timeout: 20_000 },
    );
  } catch {
    throw new Error(
      `watch reason: want ${JSON.stringify(reason)}, got ${JSON.stringify(await button.getAttribute("data-tooltip"))}`,
    );
  }
  expect(await button.getAttribute("aria-disabled")).toBe(reason === null ? null : "true");
  if (reason !== null) {
    const described = await button.getAttribute("aria-describedby");
    expect(await $(`#${described ?? ""}`).getProperty("textContent")).toBe(reason);
  }
}

/**
 * B sends an image above the server's image limit (so only a thumbnail with a full-size
 * reference travels in the message), then leaves. A opens it: the full image cannot be fetched
 * from an absent sender, and the viewer says why.
 */
async function expectFullSizeNote(expected: string): Promise<void> {
  await apps.onB(async () => {
    await pasteImage(700, 700, true);
    await $("//*[starts-with(@alt,'Image to send')]").waitForExist({ timeout: 20_000 });
    await composer().waitForEnabled();
    await browser.keys("Enter");
  });
  const thumb = $(".chat-message__body img");
  await thumb.waitForExist({ timeout: 30_000, timeoutMsg: "A never got B's image" });
  await apps.stopB();
  await browser.waitUntil(async () => !(await personRow("Bob").isExisting()), { timeout: 30_000 });
  await thumb.click();
  const viewer = $("[role='dialog'][aria-label='Image viewer']");
  await viewer.waitForDisplayed();
  const hint = viewer.$(".image-viewer__hint");
  await hint.waitForDisplayed({ timeout: 30_000, timeoutMsg: "the viewer shows no full-size note" });
  // The hint reads "Loading full size…" until the failed fetch resolves, so wait for the note.
  await browser.waitUntil(async () => (await hint.getText()).includes(expected), {
    timeout: 30_000,
    timeoutMsg: `the full-size note never said "${expected}"`,
  });
  await browser.keys("Escape");
  await viewer.waitForExist({ reverse: true });
}

(relayenv.relayAvailable() ? describe : describe.skip)("relay allowed uses (spec 07, AC-7.1 – 7.5)", function () {
  this.timeout(900_000);
  this.bail(true);

  let port = 0;

  after(async () => {
    await apps.stopB();
    await relayenv.stopRelay();
  });

  it("AC-7.1: the default relay allows everything and says so in server info", async () => {
    const relay = await relayenv.startRelay("all");
    port = relay.port;
    await completeWizard("Alice");
    await addAndConnect(`localhost:${String(port)}`);
    await joinVoice();
    const status = await relayenv.relayStatus();
    expect(status.uses).toEqual(expect.arrayContaining(["images", "files", "screen_share"]));
    expect(status.broadcast?.max_viewers).toBe(25);
    await expectRelayAllows(ALL_USES);
    await setMode("relayOnly");
    await startB("Bob", port);
    await personRow("Bob").waitForDisplayed({ timeout: 30_000 });
  });

  it("AC-7.1: nothing is held back with all uses allowed", async () => {
    await expectShareReason(null);
    await bStartsSharing();
    await expectWatch(null);
  });

  it("AC-7.4: an image whose full size cannot be fetched keeps the plain note on a relay that allows images", async () => {
    await expectFullSizeNote("Full size not available – showing the preview");
  });

  it("AC-7.3: with screen sharing off, a direct connection still works", async () => {
    const relay = await relayenv.startRelay("no-screen");
    port = relay.port;
    await expectRelayAllows("Images · Files");
    await setMode("directRelay");
    await startB("Bob", port);
    await personRow("Bob").waitForDisplayed({ timeout: 30_000 });
    await expectShareReason(null);
    await bStartsSharing();
    await expectWatch(null);
  });

  it("AC-7.3: with screen sharing off, a relay-only A can neither share nor watch", async () => {
    await setMode("relayOnly");
    await expectShareReason(SCREEN_REASON);
    // The block is read when a share is announced, so B announces its share again.
    await bStopsSharing();
    await bStartsSharing();
    await expectWatch(SCREEN_REASON);
  });

  it("AC-7.4: with images off, the note names the relay", async () => {
    await apps.stopB();
    const relay = await relayenv.startRelay("no-images");
    port = relay.port;
    await expectRelayAllows("Files · Screen sharing");
    await startB("Bob", port);
    await personRow("Bob").waitForDisplayed({ timeout: 30_000 });
    await expectFullSizeNote("Full size not available: this server's relay doesn't allow images");
  });

  it("AC-7.2: with files off, server info drops Files", async () => {
    const relay = await relayenv.startRelay("no-files");
    port = relay.port;
    await expectRelayAllows("Images · Screen sharing");
  });

  (oldImage ? it : it.skip)("AC-7.5: an old relay without allowed uses behaves as all allowed", async () => {
    const relay = await relayenv.startRelay("old", { image: oldImage, botImage: oldBotImage });
    port = relay.port;
    await expectRelayAllows(ALL_USES);
    await expectShareReason(null);
    await startB("Bob", port);
    await personRow("Bob").waitForDisplayed({ timeout: 30_000 });
    await bStartsSharing();
    await expectWatch(null);
  });
});
