import { browser, expect } from "@wdio/globals";
import { app, apps, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  finishWizard,
  joinChannel,
  messageTexts,
  reconnectSaved,
  selectChannel,
  sendText,
  waitMessage,
} from "../../helpers/chat-ui.ts";

// Issue #379: an own message keeps its place in time. Mumble never acknowledges a message, so the
// client waits a short grace period for a refusal before it counts as sent; a message someone else
// sends in that window reached the server later and must stay below it, live, after the
// confirmation and after a restart.
describe("chat: own messages stay in time order (#379)", () => {
  const first = `order-first-${String(Date.now())}`;
  const second = `order-second-${String(Date.now())}`;

  before(async () => {
    await testenv.up("v15");
    await app.restart({ freshProfile: true });
    await finishWizard("OrderA");
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
    await selectChannel("Lobby");
    await apps.startB();
    await apps.onB(async () => {
      await finishWizard("OrderB");
      await addFirstServer(testenv.ports.v15);
      await joinChannel("Lobby");
      await selectChannel("Lobby");
    });
  });

  after(async () => {
    await apps.stopB();
  });

  /** Waits until both messages are shown and confirmed, then returns their positions. */
  async function positions(): Promise<[number, number]> {
    await waitMessage(first);
    await waitMessage(second);
    await browser.waitUntil(
      async () =>
        browser.execute(() => document.querySelectorAll(".chat-message--pending").length === 0),
      { timeout: 15_000, timeoutMsg: "a message stayed pending" },
    );
    const texts = await messageTexts();
    return [
      texts.findIndex((t) => t.includes(first)),
      texts.findIndex((t) => t.includes(second)),
    ];
  }

  it("keeps a message sent by A above B's reply sent while A's was confirmed", async () => {
    await sendText(first);
    // B writes while A's message is still waiting for its refusal grace period.
    await apps.onB(() => sendText(second));
    const [a1, a2] = await positions();
    expect(a1).toBeLessThan(a2);
    const [b1, b2] = await apps.onB(positions);
    expect(b1).toBeLessThan(b2);
  });

  it("keeps the same order after a restart", async () => {
    await app.restart();
    await reconnectSaved();
    await selectChannel("Lobby");
    const [a1, a2] = await positions();
    expect(a1).toBeLessThan(a2);
  });
});
