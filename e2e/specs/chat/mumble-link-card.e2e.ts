import { browser, expect, $ } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  clickButton,
  composer,
  finishWizard,
  joinChannel,
  selectChannel,
  sleep,
  trustIfAsked,
  USER,
} from "../../helpers/chat-ui.ts";

// Issue #175: a `mumble://` link in a message gets a card below it, and nothing is connected
// until the card is used.
function lastMessage() {
  return $(
    "//ol[contains(@class,'chat-list__items')]/li[contains(@class,'chat-message')][last()]",
  );
}

async function waitForLastMessage(contains: string): Promise<void> {
  await browser.waitUntil(async () => (await lastMessage().getText()).includes(contains), {
    timeout: 20_000,
    timeoutMsg: `the last chat message does not contain ${JSON.stringify(contains)}`,
  });
}

function card(name: string) {
  return lastMessage().$(`article.mumble-card[aria-label='${name}']`);
}

describe("chat: server card for a mumble:// link (#175)", () => {
  const v15 = testenv.ports.v15;
  const v14 = testenv.ports.v14;

  before(async () => {
    await testenv.up("v15");
    await testenv.up("v14");
    await app.restart();
    await finishWizard();
    await addFirstServer(v15);
    await joinChannel("Lobby");
    await selectChannel("Lobby");
  });

  it("shows the card with the channel and the figures the query answered", async () => {
    await bot(["say", "--channel", "Lobby", `mumble://127.0.0.1:${String(v15)}/Raid/Officers`], {
      name: "LinkBot",
      port: v15,
      hold: 1,
    });
    await waitForLastMessage("Raid/Officers");
    const shown = card("Server card for 127.0.0.1");
    await shown.waitForDisplayed({ timeout: 20_000 });
    await expect(shown.$(".mumble-card__server")).toHaveText("127.0.0.1");
    await expect(shown.$(".mumble-card__channel")).toHaveText("Channel Raid / Officers");
    await expect(shown.$(".mumble-card__figures")).toHaveText(expect.stringContaining("users"));
  });

  it("warns about credentials and never shows them", async () => {
    await bot(["say", "--channel", "Lobby", `mumble://Maya:secret@127.0.0.1:${String(v15)}/Raid`], {
      name: "LinkBot",
      port: v15,
      hold: 1,
    });
    await waitForLastMessage("Raid");
    const warned = card("Server card for 127.0.0.1");
    await expect(warned.$(".mumble-card__warning")).toHaveText(
      expect.stringContaining("credentials"),
    );
    const message = lastMessage();
    expect(await message.getText()).not.toContain("secret");
    expect(await message.getText()).toContain("mumble://127.0.0.1");
  });

  it("shows at most three cards for one message", async () => {
    const links = ["A", "B", "C", "D"].map((n) => `mumble://127.0.0.1:${String(v15)}/${n}`).join(" ");
    await bot(["say", "--channel", "Lobby", links], { name: "LinkBot", port: v15, hold: 1 });
    await waitForLastMessage("/D");
    expect(await lastMessage().$$("article.mumble-card").length).toBeLessThanOrEqual(3);
  });

  it("connects only when the card's button is pressed", async () => {
    await bot(["say", "--channel", "Lobby", `mumble://127.0.0.1:${String(v14)}/Lobby`], {
      name: "LinkBot",
      port: v15,
      hold: 1,
    });
    await waitForLastMessage(`:${String(v14)}/Lobby`);
    // The second server is not saved, so the card offers to add it and nothing has connected yet.
    const shown = card("Server card for 127.0.0.1");
    await expect(shown.$(".mumble-card__join")).toHaveText("Add to servers");
    await shown.$(".mumble-card__join").click();
    const dialog = $("dialog.servers-dialog");
    await dialog.waitForDisplayed({ timeout: 15_000 });
    expect(await dialog.getText()).toContain("Save this server");
    await clickButton("Save and connect");
    await trustIfAsked();
    await sleep(6000);
    // The bot on the second server sees the user in Lobby, so the card's button really connected.
    await bot(["expect", "user", USER, "--in", "Lobby"], { name: "CheckBot", port: v14, hold: 3 });
  });

  it("previews a link typed in the composer, without a way to join from it", async () => {
    await selectChannel("Lobby");
    await composer().setValue(`mumble://127.0.0.1:${String(v15)}/Raid`);
    const preview = $(".chat-composer__links article.mumble-card");
    await preview.waitForDisplayed({ timeout: 15_000 });
    expect(await preview.$$(".mumble-card__actions button").length).toBe(0);
  });

  after(async () => {
    await testenv.stop("v15");
    await testenv.stop("v14");
  });
});
