import { createServer, type IncomingHttpHeaders, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { $, $$, browser, expect } from "@wdio/globals";
import { app, testenv } from "../../helpers/index.ts";
import { finishWizard, sleep, trustIfAsked, waitConnected } from "../../helpers/chat-ui.ts";

// AC-3.68 (issue #68): the public server browser loads its list only when it opens, pings the
// rows on screen with the standard Mumble ping, filters by name, and adds or connects a server in
// one click. The list is a local fixture served here; nothing on the internet is contacted.
const LOBBY = "Mumcord E2E Lobby";
const SILENT = "Silent E2E Room";

interface ListSite {
  server: Server;
  port: number;
  requests: IncomingHttpHeaders[];
}

/** Serves the fixture list and records the headers of every request. */
function startList(serverPort: number): Promise<ListSite> {
  const requests: IncomingHttpHeaders[] = [];
  const server = createServer((req, res) => {
    requests.push(req.headers);
    const port = (server.address() as AddressInfo).port;
    // The second entry points at this HTTP port, where no Mumble server answers the UDP ping.
    // The third is a public address the local fixture policy must drop.
    const xml = `<?xml version='1.0' standalone='yes'?>
<servers>
  <server name="${LOBBY}" ca="0" continent_code="EU" country="Germany" country_code="DE" ip="127.0.0.1" port="${String(serverPort)}" region="Test" url="https://example.org/" />
  <server name="${SILENT}" ca="0" continent_code="EU" country="Sweden" country_code="SE" ip="127.0.0.1" port="${String(port)}" region="Test" url="https://example.org/" />
  <server name="Outside World" ca="0" continent_code="NA" country="United States" country_code="US" ip="8.8.8.8" port="64738" region="Test" url="https://example.org/" />
</servers>`;
    res.writeHead(200, { "Content-Type": "text/xml; charset=utf-8" }).end(xml);
  });
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      resolve({ server, port: (server.address() as AddressInfo).port, requests });
    });
  });
}

/**
 * Row actions show on hover or keyboard focus, like the mockup. WebKitWebDriver cannot hover a
 * table row, so the test uses the keyboard: focus the button, press Enter.
 */
async function pressRowButton(label: string): Promise<void> {
  const button = await $(`button[aria-label='${label}']`).getElement();
  await button.waitForExist();
  await browser.execute((element) => {
    element.focus();
  }, button);
  await browser.keys("Enter");
}

function row(name: string) {
  return $(`//tr[contains(@class,'server-browser__row')][.//th[contains(., '${name}')]]`);
}

describe("servers: public server browser (#68)", () => {
  let site: ListSite;

  before(async () => {
    await testenv.up("v15");
    site = await startList(testenv.ports.v15);
    // The app inherits the driver's environment, so this must be set before the restart.
    process.env.MUMCORD_PUBLIC_SERVERS_URL = `http://127.0.0.1:${String(site.port)}/v1/list`;
    await app.restart({ freshProfile: true });
    await finishWizard();
  });

  after(() => {
    delete process.env.MUMCORD_PUBLIC_SERVERS_URL;
    site.server.close();
  });

  it("AC-3.68 loads the list only when the browser opens", async () => {
    const browse = $("button[aria-label='Browse public servers']");
    await browse.waitForClickable({ timeout: 30_000 });
    await sleep(1000);
    expect(site.requests).toHaveLength(0);
    await browse.click();
    await row(LOBBY).waitForDisplayed({ timeout: 20_000 });
    expect(site.requests).toHaveLength(1);
    const headers = site.requests[0];
    expect(headers["user-agent"]).toMatch(/^Mumcord\//);
    expect(headers["accept-encoding"]).toContain("gzip");
    await expect($$("tr.server-browser__row")).toBeElementsArrayOfSize(2);
  });

  it("pings the visible rows and shows ping and users", async () => {
    await expect(row(LOBBY).$(".server-browser__ping")).toHaveText(/\d+ ms/, { wait: 15_000 });
    await expect(row(LOBBY).$(".server-browser__users")).toHaveText(/\d+ \/ \d+/);
    await expect(row(SILENT).$(".server-browser__ping")).toHaveText("No answer", {
      wait: 15_000,
    });
  });

  it("AC-3.68 filters by name", async () => {
    const search = $("input.server-browser__search");
    await search.setValue("lobby");
    await expect($$("tr.server-browser__row")).toBeElementsArrayOfSize(1);
    await search.setValue("E2E");
    await expect($$("tr.server-browser__row")).toBeElementsArrayOfSize(2);
  });

  it("AC-3.68 adds a server to the favourites in one click", async () => {
    await pressRowButton(`Add ${SILENT} to your servers`);
    // Exists, not displayed: the row's actions hide again once the focused button is disabled.
    await $(`button[aria-label='${SILENT} is in your servers']`).waitForExist();
    await $(
      `//ul[@aria-label='Saved servers']//button[starts-with(@aria-label,'${SILENT}')]`,
    ).waitForExist({ timeout: 10_000 });
  });

  it("connects in one click", async () => {
    await pressRowButton(`Connect to ${LOBBY}`);
    await trustIfAsked();
    await waitConnected();
    await expect($("section.server-browser")).not.toBeExisting();
  });
});
