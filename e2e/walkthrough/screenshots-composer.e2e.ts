// Screenshots for issue #596 (chat message input bar), taken from the running client: the compact
// bar that grows with the text, the one-row icon set, the send pill, the formatting pill on the
// bar's edge and the locked note when the server refuses text messages — each in dark and light.
// One client (Alex) on the local test server in a private test environment; the AFK channel gets a
// real ACL that refuses text messages, so the locked states show server permissions and not a mock,
// and Priya (mumbot) gives the chat a second person to reply to and to mention.
//
// A runs on a display of its own at 2x scale, so the pictures come out at 2880×1800 like the other
// documentation shots. The last state narrows the window to its minimum, which drops the word
// "Send" from the button.
//
// Not part of the suite (outside specs/). Run with your own test servers, because AFK is changed:
//   MUMCORD_PROJECT=composer596 MUMCORD_PORT_BASE=41000 \
//     npm run e2e -- --spec e2e/walkthrough/screenshots-composer.e2e.ts
import type { ChildProcess } from "node:child_process";
import { join } from "node:path";
import { browser, $ } from "@wdio/globals";
import { app, repoRoot, testenv } from "../helpers/index.ts";
import { runOk } from "../helpers/run.ts";
import {
  addFirstServer,
  composer,
  finishWizard,
  joinChannel,
  pasteImage,
  ScriptBot,
  selectChannel,
  sendText,
  sleep,
  waitMessage,
} from "../helpers/chat-ui.ts";
import { both, HIDPI_ENV, openAppsMenu, startHidpiDisplay } from "./shots.ts";

const TYPING =
  "Found it: the crash only happens when the relay restarts.\n" +
  "Can someone on Windows try to reproduce it?";
const LONG =
  "Found it: the crash only happens when the relay restarts while a client is still muted.\n" +
  "I attached the log from my machine.\n" +
  "Can someone on Windows try to reproduce it with the 0.7.1 build?\n" +
  "Steps: join the channel, mute, wait for the relay restart, unmute.\n" +
  "After that the audio never comes back until I reconnect.\n" +
  "Happy to test any build you throw at me, just ping me here and I will answer as soon as I see" +
  " it.\n" +
  "Thanks!";
const PRIYA = "Anyone tried the Windows build against the 0.7.1 crash yet?";

/** Mumble's ACL bits, as in `crates/protocol/src/permissions.rs`. */
const TRAVERSE = 0x2;
const ENTER = 0x4;
const SPEAK = 0x8;
const WHISPER = 0x100;
const TEXT_MESSAGE = 0x200;
const LISTEN = 0x800;

const project = process.env.MUMCORD_PROJECT ?? "testenv";
const database = join(
  repoRoot,
  "testenv",
  project === "testenv" ? "data" : join("data", `_${project}`),
  "v15",
  "mumble-server.sqlite",
);

/**
 * AFK stops inheriting the server's ACL and gets its own: entering and speaking stay allowed for
 * everyone, text messages are refused. Written while the server is stopped, because murmur reads
 * channels and ACLs only at start-up.
 */
async function refuseTextInAfk(): Promise<void> {
  await testenv.stop("v15");
  await runOk("sqlite3", [
    database,
    [
      "update channels set inheritacl = 0 where name = 'AFK';",
      "delete from acl where channel_id = (select channel_id from channels where name = 'AFK');",
      "insert or replace into acl",
      "  (server_id, channel_id, priority, user_id, group_name, apply_here, apply_sub, grantpriv, revokepriv)",
      "values (1, (select channel_id from channels where name = 'AFK' limit 1), 1, null, 'all', 1, 1,",
      `  ${String(TRAVERSE | ENTER | SPEAK | WHISPER | LISTEN)}, ${String(TEXT_MESSAGE)});`,
    ].join(" "),
  ]);
  await testenv.start("v15");
  await sleep(5_000);
}

/**
 * Puts `text` into the composer. WebDriver's own `setValue` would type the newlines as Enter,
 * which sends the message, and it refuses an empty string — so the value is set whole, through the
 * input event React listens to.
 */
async function setComposer(text: string): Promise<void> {
  await browser.execute((value: string) => {
    const field = document.querySelector<HTMLTextAreaElement>("textarea[aria-label^='Message']");
    if (!field) throw new Error("no composer");
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set?.call(
      field,
      value,
    );
    // WebKit leaves the caret at the start of a field it did not type into; the mention list reads
    // the caret, so put it behind the text the way a paste would.
    field.setSelectionRange(value.length, value.length);
    field.dispatchEvent(new Event("input", { bubbles: true }));
    field.dispatchEvent(new Event("select", { bubbles: true }));
  }, text);
}

/** Clicks into the field and shows `text`, so the bar is in its focused, grown state. */
async function type(text: string): Promise<void> {
  const box = composer();
  await box.waitForDisplayed();
  await box.click();
  await setComposer(text);
  await sleep(800);
}

/** Takes the pointer to the empty server bar, so no tooltip stays in the picture. */
async function parkPointer(): Promise<void> {
  await browser.action("pointer").move({ x: 24, y: 600 }).perform();
  await sleep(300);
}

/** Empties the tray: a picture from one state would otherwise still sit in the next one's bar. */
async function clearTray(): Promise<void> {
  for (;;) {
    const remove = $("button[aria-label^='Remove image']");
    if (!(await remove.isExisting().catch(() => false))) return;
    await remove.click();
    await sleep(400);
  }
}

/** The chat of Lobby, with the field and the tray cleared. */
async function lobbyChat(): Promise<void> {
  await selectChannel("Lobby");
  await setComposer("");
  await clearTray();
  await sleep(400);
}

/** The bar with the text field unfocused (nothing else takes the focus). */
async function unfocused(): Promise<void> {
  await browser.execute(() => {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
  });
  await sleep(400);
}

describe("chat input bar screenshots (issue #596)", function () {
  this.timeout(40 * 60_000);
  this.bail(true);

  let xvfb: ChildProcess | undefined;
  let priya: ScriptBot;

  before(async function () {
    this.timeout(20 * 60_000);
    xvfb = await startHidpiDisplay();
    await testenv.up("v15");
    await refuseTextInAfk();
    await app.restart({ freshProfile: true, env: { ...HIDPI_ENV } });
    await browser.setWindowSize(1440, 900);
    await finishWizard("Alex");
    await addFirstServer(testenv.ports.v15);
    // Alex is in Lobby by voice: that is what delivers the channel's chat to the client, and what
    // makes the Apps button belong to the icon row.
    await joinChannel("Lobby");
    // Priya speaks the channel, so there is a second person to reply to and to mention.
    priya = new ScriptBot({ name: "Priya", port: testenv.ports.v15 });
    await priya.connected();
    await priya.run("join Lobby");
    await priya.run(`say --channel Lobby "${PRIYA}"`);
    await waitMessage("0.7.1 crash yet");
    await sendText("Evening! Did anyone look into the crash from last night yet?");
    await sleep(1_500);
  });

  after(() => {
    priya.stop();
    xvfb?.kill();
  });

  it("no Apps button where the user is not in the channel by voice", async () => {
    await both("chat-input-no-voice", async () => {
      await selectChannel("Raid");
      await setComposer("");
      await sleep(400);
    });
  });

  it("empty, not focused", async () => {
    await both("chat-input-empty", async () => {
      await lobbyChat();
      await unfocused();
    });
  });

  it("empty, but focused: the bar makes room for the formatting pill", async () => {
    await both("chat-input-empty-focused", async () => {
      await lobbyChat();
      await composer().click();
      await sleep(600);
    });
  });

  it("typing, multiline, grown with the text", async () => {
    await both("chat-input-typing", async () => {
      await lobbyChat();
      await type(TYPING);
    });
  });

  it("at the maximum height: six lines, the rest scrolls", async () => {
    await both("chat-input-max-height", async () => {
      await lobbyChat();
      await type(LONG);
    });
  });

  it("with an attachment: send is live, the text is optional", async () => {
    await both("chat-input-attachment", async () => {
      await lobbyChat();
      const box = composer();
      await box.waitForDisplayed();
      await box.click();
      await pasteImage(640, 480, false);
      await sleep(2_500);
      await parkPointer();
      await sleep(400);
    });
  });

  it("keyboard focus on an icon button: the pill stays, the bar keeps its plain frame", async () => {
    await both("chat-input-button-focus", async () => {
      await lobbyChat();
      await type("Evening");
      const apps = $("button.chat-apps__button");
      await apps.click();
      await sleep(400);
      await browser.keys("Escape"); // closes the menu and hands the focus back to the button
      await parkPointer();
      await sleep(600);
    });
  });

  it("Apps menu open while typing: the formatting pill makes way", async () => {
    await both(
      "chat-input-apps-menu",
      async () => {
        await lobbyChat();
        await type("Evening — sharing the log now.");
        await openAppsMenu();
        await parkPointer();
        await sleep(600);
      },
      true,
    );
  });

  it("mention list while typing, beside the formatting pill", async () => {
    // `keep`: the theme switch presses Escape, and Escape over the open list would dismiss this
    // very `@query` for good — the second theme would then never show the list again.
    await both(
      "chat-input-mention",
      async () => {
        await lobbyChat();
        const box = composer();
        await box.waitForDisplayed();
        await box.click();
        // Typed for real here: the list follows the caret, and only key presses move it the way
        // React reads it.
        await box.setValue("ping @P");
        await $("div[role='listbox']").waitForDisplayed({ timeout: 10_000 });
        await parkPointer();
        await sleep(600);
      },
      true,
    );
  });

  it("replying to a message", async () => {
    await both("chat-input-reply", async () => {
      await lobbyChat();
      const message = $(`//li[contains(@class,'chat-message')][contains(.,'0.7.1 crash yet')]`);
      await message.waitForDisplayed();
      await message.moveTo();
      await sleep(400);
      // Only the hovered message shows its action row; the other rows keep their buttons hidden.
      const reply = message.$("button[aria-label^='Reply to']");
      await reply.waitForDisplayed({ timeout: 10_000 });
      await reply.click();
      await parkPointer();
      await sleep(800);
    });
  });

  it("no write permission: a locked note instead of the input", async () => {
    await both("chat-input-locked", async () => {
      await selectChannel("AFK");
      await sleep(800);
    });
  });

  it("no write permission, but in the channel by voice: Apps stays active", async () => {
    await both("chat-input-locked-apps", async () => {
      await joinChannel("AFK");
      await selectChannel("AFK");
      await sleep(800);
    });
  });

  it("narrow window: the icon row keeps its place, the word Send drops", async () => {
    await both("chat-input-narrow", async () => {
      await browser.setWindowSize(800, 900);
      await selectChannel("Lobby");
      await type("On the road: bar stays usable.");
    });
  });
});
