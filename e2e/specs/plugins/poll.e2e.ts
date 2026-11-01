// Poll between two Mumcord clients and a plain Mumble user (spec 06 AC-6.2, AC-6.3; C2).
// A and B install Poll from the fixture registry; the bot "Carol" stands in for an official
// Mumble client: she only sees the "[Poll] …" text line and votes with "!poll <n>".
// A installs Poll without `chat.triggers`, so A's poll first shows the "Turn on !poll voting"
// button (P8), which leads to Poll's permissions in Settings.
import { $, browser, expect } from "@wdio/globals";
import { app, apps, registry, testenv } from "../../helpers/index.ts";
import { joinChannel, messageTexts, ScriptBot, selectChannel } from "../../helpers/chat-ui.ts";
import { openSettingsGroup, settingsCard } from "../../helpers/settings-ui.ts";
import { addAndConnect, completeWizard, reconnect } from "../../helpers/voice-ui.ts";

const CHANNEL = "Lobby";
const QUESTION = "Pizza or pasta?";
const OPTIONS = ["Pizza", "Pasta", "Salad", "Soup"];
const TRIGGERS_TEXT = "Read chat messages that start with its trigger words";
/** AC-6.2: tallies update within 3 s. */
const TALLY_MS = 3_000;

/** The chat line (list item) that holds Poll's card. */
const cardItem = () =>
  $("//li[.//section[contains(@class,'plugin-chat-card')][@aria-label='Card from Poll']]");
const sheet = () => $(".pstrip-sheet");

/** Text the user can see anywhere in the window (hidden elements excluded). */
async function visibleText(): Promise<string> {
  return browser.execute(() => document.body.innerText);
}

/** Visible text of the open chat's messages (screen-reader announcements excluded). */
async function chatText(): Promise<string> {
  return (await messageTexts()).join("\n");
}

async function waitVisible(text: string, timeout = 20_000): Promise<void> {
  await browser.waitUntil(async () => (await visibleText()).includes(text), {
    timeout,
    timeoutMsg: `${JSON.stringify(text)} never showed`,
  });
}

/**
 * Installs Poll from Browse; `triggers: false` unticks the suggested `chat.triggers`. The fixture
 * registry is plain http on 127.0.0.1, which only Developer mode allows (FR-6.74).
 */
async function installPoll(triggers: boolean): Promise<void> {
  await openSettingsGroup("Advanced & Developer");
  const developer = settingsCard("Developer mode").$("input[role='switch']");
  await developer.waitForExist({ timeout: 10_000 });
  if (!(await developer.isSelected())) await developer.click();
  await expect(developer).toBeSelected({ wait: 5_000 });
  await openSettingsGroup("Plugins");
  await $("//button[@role='tab'][normalize-space(.)='Browse']").click();
  const install = $("button[aria-label='Install Poll']");
  await install.waitForClickable({ timeout: 30_000 });
  await install.click();
  const prompt = "//div[contains(@class,'plugin-prompt')]";
  const confirm = $(`${prompt}//button[normalize-space(.)='Install']`);
  await confirm.waitForClickable({ timeout: 30_000 });
  const toggle = $(`${prompt}//label[contains(@class,'perm-label')][contains(., '${TRIGGERS_TEXT}')]`);
  const box = toggle.$("input");
  if ((await box.isSelected()) !== triggers) await toggle.click();
  await browser.waitUntil(async () => (await box.isSelected()) === triggers, {
    timeout: 5_000,
    timeoutMsg: "the chat.triggers switch did not change",
  });
  await confirm.click();
  await expect($(".plugin-toast")).toHaveText(expect.stringContaining("Poll installed"), {
    wait: 30_000,
  });
  await browser.keys("Escape");
  await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
}

/** Wizard, connect, join the poll channel and show its chat. */
async function joinAs(name: string): Promise<void> {
  await completeWizard(name);
  await addAndConnect();
  await joinChannel(CHANNEL);
  await selectChannel(CHANNEL);
}

/** Opens Poll's inline panel in the Plugins strip (no-op when it is open). */
async function openPollPanel(): Promise<void> {
  const icon = $(".pstrip-icons button[aria-label='Poll']");
  await icon.waitForClickable({ timeout: 30_000 });
  if (!(await $(".pstrip-panel").isExisting())) await icon.click();
  await $(".pstrip-panel").waitForDisplayed({ timeout: 10_000 });
}

/** A text field of a plugin form, by its label. */
function field(label: string) {
  return $(`//div[contains(@class,'pr-field')][label[normalize-space(.)='${label}']]//input`);
}

describe("Poll with two Mumcord clients and a plain Mumble user", () => {
  let fixtures: registry.FixtureRegistry | undefined;
  let carol: ScriptBot | undefined;

  before(async function () {
    this.timeout(20 * 60_000);
    await testenv.up("v15");
    fixtures = await registry.start();
  });

  const env = () => {
    if (fixtures === undefined) throw new Error("the fixture registry did not start");
    return fixtures.env();
  };
  const bot = () => {
    if (carol === undefined) throw new Error("Carol is not connected");
    return carol;
  };

  after(async () => {
    carol?.stop();
    await apps.stopB();
    await fixtures?.stop();
  });

  it("A and B install Poll and join the channel with a plain Mumble user", async () => {
    await app.restart({ env: env(), freshProfile: true });
    await joinAs("Alice");
    await installPoll(false);
    await apps.startB({ env: env() });
    await apps.onB(async () => {
      await joinAs("Bob");
      await installPoll(true);
    });
    // A peer's plugin list is announced at most once per 10 s (FR-6.35): give B's announce of
    // the new Poll time to reach A before A creates the poll.
    await browser.pause(11_000);
    carol = new ScriptBot({ name: "Carol", port: testenv.ports.v15, timeout: 60 });
    await carol.connected();
    await carol.run(`join ${CHANNEL}`);
  });

  it("AC-6.2: A creates a poll; Carol gets the text line, B the compact card in its place", async () => {
    await openPollPanel();
    await $(".pstrip-panel").$("button=New poll").click();
    await sheet().waitForDisplayed({ timeout: 10_000 });
    await field("Question").setValue(QUESTION);
    for (const [i, option] of OPTIONS.entries()) {
      if (i >= 2) await sheet().$("button=Add option").click();
      await field(`Option ${String(i + 1)}`).waitForDisplayed({ timeout: 5_000 });
      await field(`Option ${String(i + 1)}`).setValue(option);
    }
    await sheet().$("button=Create poll").click();
    await waitVisible("Your poll · 0 votes");

    await bot().run(`expect message --from Alice --channel ${CHANNEL} --contains "[Poll]"`);
    await bot().run(`expect message --from Alice --contains "${QUESTION}"`);
    await bot().run('expect message --from Alice --contains "!poll"');

    await apps.onB(async () => {
      await cardItem().waitForDisplayed({ timeout: 20_000 });
      const item = cardItem();
      // H2: the card takes the line's place, marked with Poll's icon; the text is one click away.
      await expect(item.$("img.plugin-chat-mark")).toExist();
      const show = item.$("button=Show original text");
      await expect(show).toBeDisplayed();
      expect(await chatText()).not.toContain("[Poll]");
      // P4: compact card with the top three options and "Open".
      const card = item.$("section.plugin-chat-card");
      await expect(card.$("button=Open")).toBeDisplayed();
      // textContent: the card's body may be faded at its height limit ("Show more").
      const cardText = await card.getProperty("textContent").then(String);
      expect(cardText).toContain(QUESTION);
      for (const option of OPTIONS.slice(0, 3)) expect(cardText).toContain(option);
      expect(cardText).not.toContain(OPTIONS[3]);
      await show.click();
      await expect(item).toHaveText(expect.stringContaining("[Poll]"));
      await item.$("button=Hide original text").click();
      expect(await chatText()).not.toContain("[Poll]");
    });
  });

  it("P8: without chat.triggers A's poll offers to turn on !poll voting", async () => {
    await openPollPanel();
    await $(".pstrip-panel .pstrip-expand").click();
    await sheet().waitForDisplayed({ timeout: 10_000 });
    const hint = sheet().$("button=Turn on !poll voting in the plugin permissions");
    await hint.waitForClickable({ timeout: 10_000 });
    await hint.click();
    const toggle = $(`//label[contains(@class,'perm-label')][contains(., '${TRIGGERS_TEXT}')]`);
    await toggle.waitForDisplayed({ timeout: 15_000 });
    await expect(toggle.$("input")).not.toBeSelected();
    await toggle.click();
    await expect(toggle.$("input")).toBeSelected({ wait: 10_000 });
    await browser.keys("Escape");
    await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
    // The side sheet stays open behind Settings; reopen it only if it closed.
    if (!(await sheet().isDisplayed())) {
      await openPollPanel();
      await $(".pstrip-panel .pstrip-expand").click();
    }
    await sheet().waitForDisplayed({ timeout: 10_000 });
    await waitVisible("Your poll · 0 votes");
    await browser.waitUntil(
      async () => !(await visibleText()).includes("Turn on !poll voting in the plugin permissions"),
      { timeout: 10_000, timeoutMsg: "the !poll hint stayed after granting chat.triggers" },
    );
  });

  it("AC-6.2: Carol's !poll is hidden and counted; B's vote reaches both tallies within 3 s", async () => {
    await bot().run(`say --channel ${CHANNEL} "!poll 2"`);
    await waitVisible("Your poll · 1 vote");
    await waitVisible("Hidden by Poll");
    expect(await chatText()).not.toContain("!poll 2");
    await apps.onB(async () => {
      await waitVisible("Hidden by Poll");
      expect(await chatText()).not.toContain("!poll 2");
      await waitVisible("Poll by Alice · 1 vote");
      await cardItem().$("button=Open").click();
      await sheet().waitForDisplayed({ timeout: 10_000 });
      await sheet().$(`button=${OPTIONS[0]}`).click();
      await waitVisible("Poll by Alice · 2 votes", TALLY_MS);
    });
    await waitVisible("Your poll · 2 votes", TALLY_MS);
  });

  it("AC-6.3: B leaves and rejoins and gets the current poll", async () => {
    await apps.restartB();
    await apps.onB(async () => {
      await reconnect();
      await joinChannel(CHANNEL);
      await selectChannel(CHANNEL);
      // The vote count only comes with the creator's resend, not from the chat history.
      await waitVisible("Poll by Alice · 2 votes", 30_000);
    });
  });

  it("AC-6.3: A disconnects and B sees the poll frozen", async () => {
    await app.restart();
    await apps.onB(async () => {
      await waitVisible("Creator left, poll is frozen", 30_000);
    });
  });
});
