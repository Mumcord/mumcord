import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { expect, $ } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  expectBlobImage,
  finishWizard,
  joinChannel,
  ScriptBot,
  selectChannel,
  sendText,
  waitMessage,
} from "../../helpers/chat-ui.ts";

/** 1×1 red PNG as the card's graph. */
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC",
  "base64",
);

/** Local image host; records every requested path. No internet in tests. */
function startSite(): Promise<{ server: Server; port: number; requests: string[] }> {
  const requests: string[] = [];
  const server = createServer((req, res) => {
    requests.push(req.url ?? "/");
    res.writeHead(200, { "Content-Type": "image/png" }).end(PNG);
  });
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      resolve({ server, port: (server.address() as AddressInfo).port, requests });
    });
  });
}

function card(name: string) {
  return $(`section.message-card[aria-label='${name}']`);
}

// Issue #72: message cards from bots and typed BB code, and the fallback for official clients.
describe("chat: message cards (#72)", () => {
  let site: Awaited<ReturnType<typeof startSite>>;

  before(async () => {
    site = await startSite();
    await testenv.up("v15");
    await app.restart();
    await finishWizard();
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
    await selectChannel("Lobby");
  });

  after(() => {
    site.server.close();
  });

  it("shows a bot's card with status, text, the image through the engine and the link", async () => {
    const image = `http://127.0.0.1:${String(site.port)}/cpu.png`;
    const fields = new URLSearchParams({
      v: "1",
      title: "CPU high on db-1",
      color: "warning",
      icon: "chart",
      image,
      url: "https://grafana.test/d/db-1",
    });
    const html =
      `<div data-mumcord-card="${fields.toString().replace(/&/g, "&amp;")}">` +
      "<b>[Warning] CPU high on db-1</b>" +
      '<div data-mumcord-card="body">Load <b>97 %</b> for 5 min</div>' +
      `Image: <a href="${image}">${image}</a><br>` +
      '<a href="https://grafana.test/d/db-1">https://grafana.test/d/db-1</a></div>';
    await bot(["say", "--channel", "Lobby", html], { name: "MonitorBot", hold: 1 });
    const shown = card("Card: CPU high on db-1, Warning");
    await shown.waitForDisplayed({ timeout: 15_000 });
    await expect(shown.$(".message-card__status")).toHaveText(expect.stringContaining("Warning"));
    await expect(shown.$(".message-card__body b")).toHaveText("97 %");
    await expect(shown.$(".message-card__icon")).toExist();
    await expectBlobImage(shown.$("button.message-card__image img"));
    expect(site.requests).toContain("/cpu.png");
    await expect(shown.$(".message-card__link a[href='https://grafana.test/d/db-1']")).toExist();
    await expect(shown.$("button.message-card__open")).toExist();
    // The fallback lines for official clients stay hidden in Mumcord.
    await expect($("//li[contains(@class,'chat-message')][contains(., '[Warning]')]")).not.toExist();
  });

  it("renders card BB code typed in an official client as a card", async () => {
    await bot(
      ["say", "--channel", "Lobby", "[card title=&quot;Backup failed&quot; color=error]db-1 is <b>down</b>[/card]"],
      { name: "OfficialUser", hold: 1 },
    );
    const shown = card("Card: Backup failed, Error");
    await shown.waitForDisplayed({ timeout: 15_000 });
    await expect(shown.$(".message-card__body b")).toHaveText("down");
  });

  it("sends a typed card as HTML that official clients read as formatted text", async () => {
    const listener = new ScriptBot({ name: "Classic" });
    await listener.connected();
    await listener.run("join Lobby");
    try {
      await sendText(
        "[card title=\"Disk 85 % on files-1\" color=warning url=https://grafana.test/d/files-1]About **3 days** left[/card]",
      );
      const got = await listener.run('expect message --channel Lobby --contains "Disk 85"');
      expect(got).toContain("<b>[Warning] Disk 85 % on files-1</b>");
      expect(got).toContain("About <strong>3 days</strong> left");
      expect(got).toContain(
        '<a href="https://grafana.test/d/files-1">https://grafana.test/d/files-1</a>',
      );
      expect(got).toContain('data-mumcord-card="v=1&amp;title=Disk+85+%25+on+files-1');
      expect(got).not.toContain("[card");
    } finally {
      listener.stop();
    }
    await waitMessage("Disk 85 % on files-1");
    await card("Card: Disk 85 % on files-1, Warning").waitForDisplayed();
  });
});
