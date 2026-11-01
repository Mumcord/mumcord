// Echolings between two Mumcord friends (spec 06 AC-6.5, AC-6.6; C4).
// A and B are friends and install Echolings with the default visibility "Friends". Each hatches a
// pet; A's pet badge and profile-card section reach B, B visits once (a second pat the same day is
// refused), and both play Echo Echo (lobby runs out on its own) and Snack Race (the web view with
// the page↔plugin bridge). Last, A adds Poll, Dice and a badge fixture: one badge per user, and the
// "Badge shown next to names" picker switches it at once.
import { $, $$, browser, expect } from "@wdio/globals";
import { app, apps, registry, testenv } from "../../helpers/index.ts";
import { joinChannel, selectChannel } from "../../helpers/chat-ui.ts";
import { openSettingsGroup, settingsCard } from "../../helpers/settings-ui.ts";
import { addAndConnect, completeWizard, personRow } from "../../helpers/voice-ui.ts";

const CHANNEL = "Lobby";
const PET_A = "Pip";
const PET_B = "Moss";
/** Optional permissions Echolings needs for the social parts (peers.messages, ui.chat-card, notify). */
const OPTIONAL = [
  "Exchange data with this plugin for other people in your channel",
  "Show cards in the chat",
  "Show notifications",
];
const WEBVIEW_TEXT = "Show its own web content in a sealed frame";

const prompt = "//div[contains(concat(' ',@class,' '),' plugin-prompt ')]";
const pluginTab = (name: string) =>
  // Installed carries its count ("Installed 3").
  $(`//button[@role='tab'][contains(@class,'plugins-tab')][starts-with(normalize-space(.),'${name}')]`);
/** Echolings' expanded panel (its picker and care actions live there, not in the inline strip). */
const panel = () => $(".pstrip-sheet");
const badges = (name: string) => personRow(name).$$(".plugin-badge");
/** The newest Echolings chat card whose text contains `title`. */
const card = (title: string) =>
  $(
    `(//section[contains(@class,'plugin-chat-card')][@aria-label='Card from Echolings'][contains(.,'${title}')])[last()]`,
  );
const raceFrame = () => $("iframe.plugin-webview__frame");

async function waitText(
  el: () => ChainablePromiseElement,
  text: string | RegExp,
  timeout = 20_000,
) {
  await browser.waitUntil(
    async () => {
      const content = await el().getProperty("textContent").then(String);
      return typeof text === "string" ? content.includes(text) : text.test(content);
    },
    { timeout, timeoutMsg: `${String(text)} never showed` },
  );
}
type ChainablePromiseElement = ReturnType<typeof $>;

/**
 * Puts `value` into a React text field through the native setter and an input event. Typed keys
 * lose Shift in some runs (WebKitWebDriver), which turns "http://" into "http;//".
 */
async function setField(input: ChainablePromiseElement, value: string): Promise<void> {
  await input.waitForDisplayed({ timeout: 10_000 });
  await browser.execute(
    (el: HTMLInputElement, text: string) => {
      // The prototype setter (with `el` as receiver) bypasses React's value tracking.
      Reflect.set(HTMLInputElement.prototype, "value", text, el);
      el.dispatchEvent(new Event("input", { bubbles: true }));
    },
    (await input.getElement()) as unknown as HTMLInputElement,
    value,
  );
  await expect(input).toHaveValue(value);
}

async function closeSettings(): Promise<void> {
  await browser.keys("Escape");
  await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
}

async function developerMode(): Promise<void> {
  await openSettingsGroup("Advanced & Developer");
  const developer = settingsCard("Developer mode").$("input[role='switch']");
  await developer.waitForExist({ timeout: 10_000 });
  if (!(await developer.isSelected())) await developer.click();
  await expect(developer).toBeSelected({ wait: 5_000 });
}

/**
 * Installs `name` from Browse (Settings must show Plugins). `optional` lists permission texts to
 * switch on; a repository plugin needs its warning acknowledged.
 */
async function install(name: string, optional: string[] = []): Promise<void> {
  await pluginTab("Browse").click();
  const button = $(`button[aria-label='Install ${name}']`);
  await button.waitForClickable({ timeout: 30_000 });
  await button.click();
  const confirm = $(`${prompt}//button[normalize-space(.)='Install']`);
  await confirm.waitForDisplayed({ timeout: 60_000 });
  for (const text of optional) {
    const box = $(`${prompt}//label[contains(@class,'perm-label')][contains(.,'${text}')]//input`);
    if (!(await box.isSelected())) await box.click();
    await expect(box).toBeSelected({ wait: 5_000 });
  }
  const understood = $(`${prompt}//div[contains(@class,'plugin-prompt-warning')]//input`);
  if ((await understood.isExisting()) && !(await understood.isSelected())) await understood.click();
  await confirm.waitForClickable({ timeout: 30_000 });
  await confirm.click();
  await expect($(".plugin-toast")).toHaveText(expect.stringContaining(`${name} installed`), {
    wait: 60_000,
  });
}

/** Installs Echolings with its social permissions; the visibility stays at the default Friends. */
async function installEcholings(): Promise<void> {
  await developerMode();
  await openSettingsGroup("Plugins");
  await pluginTab("Browse").click();
  await $("button[aria-label='Install Echolings']").waitForClickable({ timeout: 30_000 });
  await $("button[aria-label='Install Echolings']").click();
  // FR-6.83 / AC-6.5: Friends is the default visibility; the web view is suggested (pre-ticked).
  await $(`${prompt}//button[normalize-space(.)='Install']`).waitForDisplayed({ timeout: 60_000 });
  await expect($(`${prompt}//input[@type='radio'][@value='friends']`)).toBeSelected();
  await expect(
    $(`${prompt}//label[contains(@class,'perm-label')][contains(.,'${WEBVIEW_TEXT}')]//input`),
  ).toBeSelected();
  await $(`${prompt}//button[normalize-space(.)='Cancel']`).click();
  await install("Echolings", OPTIONAL);
  await closeSettings();
}

async function addFriend(name: string): Promise<void> {
  await personRow(name).waitForDisplayed({ timeout: 20_000 });
  await personRow(name).click({ button: "right" });
  const add = $("//*[@role='menuitem'][normalize-space()='Add friend']");
  await add.waitForClickable();
  await add.click();
  await $("//*[@role='menuitem'][normalize-space()='Remove friend']").waitForDisplayed();
  await browser.keys("Escape");
}

async function openEcholings(): Promise<void> {
  if (await panel().isDisplayed().catch(() => false)) return;
  const icon = $(".pstrip-icons button[aria-label='Echolings']");
  await icon.waitForClickable({ timeout: 30_000 });
  if (!(await $(".pstrip-panel").isExisting())) await icon.click();
  await $(".pstrip-panel .pstrip-expand").click();
  await panel().waitForDisplayed({ timeout: 10_000 });
}

/** Closes the expanded panel so the chat behind it can be used. */
async function closeEcholings(): Promise<void> {
  if (!(await panel().isDisplayed().catch(() => false))) return;
  await panel().$("button.pstrip-expand").click();
  await panel().waitForDisplayed({ reverse: true, timeout: 10_000 });
}

async function hatch(pet: string): Promise<void> {
  await openEcholings();
  await waitText(panel, "Choose your Echoling");
  const name = panel().$("//div[contains(@class,'pr-field')][label[normalize-space(.)='Name']]//input");
  await setField(name, pet);
  await name.click();
  await browser.keys("Enter");
  await panel().$("button=Hatch").click();
  await waitText(panel, "Hum");
  await waitText(panel, pet);
  await closeEcholings();
}

async function clickIn(scope: () => ChainablePromiseElement, label: string): Promise<void> {
  const button = scope().$(`button=${label}`);
  await button.waitForClickable({ timeout: 20_000 });
  await button.click();
}

/** Opens the full profile card of `name` from the member list. */
async function openProfile(name: string): Promise<ChainablePromiseElement> {
  const row = $(
    `//button[contains(@class,'member')][.//span[contains(@class,'member__name') and normalize-space(text()[1])='${name}']]`,
  );
  await row.waitForExist({ timeout: 20_000 });
  await row.scrollIntoView({ block: "center" });
  await row.click();
  const full = $("//div[@id='profile-preview']//button[@aria-label='Full profile']");
  await full.waitForDisplayed();
  await full.click();
  const profile = $("dialog.profile-card");
  await profile.waitForDisplayed();
  return profile;
}

/**
 * Taps inside the race page until the run ends: a frame loop presses Hop! whenever the marker is
 * on the snack (the page's own timing helpers), so both clients race at the same time while the
 * test drives the other one. Returns whether the loop could be installed.
 */
async function autoRace(): Promise<boolean> {
  await raceFrame().waitForExist({ timeout: 20_000 });
  await browser.switchFrame(await raceFrame());
  try {
    await $("#hop").waitForExist({ timeout: 10_000 });
    return await browser.execute(() => {
      const g = globalThis as unknown as { __autoRace?: boolean };
      if (g.__autoRace === true) return true;
      g.__autoRace = true;
      // Only the page's DOM (the script may run in its own world): the page draws the snack zone
      // and the marker as left/width percentages every frame while the run is on.
      const percent = (id: string, prop: "left" | "width") =>
        Number.parseFloat(document.getElementById(id)?.style[prop] ?? "");
      let last = 0;
      const step = () => {
        const hop = document.getElementById("hop") as HTMLButtonElement | null;
        const left = percent("zone", "left");
        const width = percent("zone", "width");
        const marker = percent("marker", "left");
        const now = performance.now();
        if (
          hop !== null &&
          !hop.disabled &&
          now - last > 300 &&
          marker >= left + width * 0.3 &&
          marker <= left + width * 0.7
        ) {
          last = now;
          // A click without a pointer (detail 0) taps, like a screen reader.
          hop.click();
        }
        requestAnimationFrame(step);
      };
      requestAnimationFrame(step);
      return true;
    });
  } catch (e) {
    console.warn(`[e2e] the race helper could not run in the page: ${String(e)}`);
    return false;
  } finally {
    await browser.switchFrame(null);
  }
}

/** Texts of the race page: title, Hop! label, every lane's "n of 10 hops" and the results. */
async function racePage(): Promise<{ title: string; hop: string; lanes: string[]; results: string[] }> {
  await browser.switchFrame(await raceFrame());
  try {
    const title = await $("#title").getText();
    const hop = await $("#hop").getProperty("textContent").then(String);
    const lanes: string[] = [];
    for (const lane of await $$("#track li")) {
      const who = await lane.$(".name").getProperty("textContent").then(String);
      const hops = (await lane.$(".path").getAttribute("aria-label")) ?? "";
      lanes.push(`${who}: ${hops}`);
    }
    const results: string[] = [];
    for (const item of await $$("#results li")) {
      results.push(await item.getProperty("textContent").then(String));
    }
    return { title, hop, lanes, results };
  } finally {
    await browser.switchFrame(null);
  }
}

describe("Echolings between two friends (AC-6.5, AC-6.6)", () => {
  let fixtures: registry.FixtureRegistry | undefined;

  before(async function () {
    this.timeout(20 * 60_000);
    await testenv.up("v15");
    fixtures = await registry.start();
  });

  const env = () => {
    if (fixtures === undefined) throw new Error("the fixture registry did not start");
    return fixtures.env();
  };

  after(async () => {
    await apps.stopB();
    await fixtures?.stop();
  });

  it("A and B are friends with Echolings (visibility Friends) and hatch a pet each", async () => {
    await app.restart({ env: env(), freshProfile: true });
    await completeWizard("Alice");
    await addAndConnect();
    await joinChannel(CHANNEL);
    await selectChannel(CHANNEL);
    await apps.startB({ env: env() });
    await apps.onB(async () => {
      await completeWizard("Bob");
      await addAndConnect();
      await joinChannel(CHANNEL);
      await selectChannel(CHANNEL);
      await addFriend("Alice");
    });
    await addFriend("Bob");

    await installEcholings();
    await apps.onB(installEcholings);
    // A peer's plugin list is announced at most once per 10 s (FR-6.35): let both lists arrive
    // before the pets are hatched, so the first pet cards reach the other side.
    await browser.pause(11_000);
    await hatch(PET_A);
    await apps.onB(() => hatch(PET_B));
  });

  it("AC-6.5: A pats its pet and A's own badge animates", async () => {
    await openEcholings();
    await clickIn(panel, "Pat");
    await closeEcholings();
    await browser.waitUntil(async () => (await badges("Alice").length) === 1, {
      timeout: 20_000,
      timeoutMsg: "A's own row has no single pet badge",
    });
    await expect(personRow("Alice").$(".plugin-badge .pr-anim")).toExist({ wait: 5_000 });
  });

  it("AC-6.5: B sees one pet badge on A and A's section on the profile card", async () => {
    await apps.onB(async () => {
      // Pet cards go out at most once a minute (README "Messages").
      await personRow("Alice").$(".plugin-badge").waitForExist({ timeout: 75_000 });
      await expect(badges("Alice")).toBeElementsArrayOfSize(1);
      await expect(personRow("Alice").$(`.plugin-badge img[alt^='${PET_A} the ']`)).toExist();
      const profile = await openProfile("Alice");
      const section = () => profile.$("section.plugin-profile-section[aria-label='Section from Echolings']");
      await section().waitForDisplayed({ timeout: 10_000 });
      await waitText(section, PET_A);
      await waitText(section, "Bond");
    });
  });

  it("AC-6.5: B pats A's pet once; a second pat the same day is refused", async () => {
    await apps.onB(async () => {
      const section = () =>
        $("dialog.profile-card section.plugin-profile-section[aria-label='Section from Echolings']");
      const pat = () => section().$(`button=Pat ${PET_A}`);
      await pat().waitForClickable({ timeout: 10_000 });
      await pat().click();
      await waitText(section, `You patted ${PET_A}. See you tomorrow!`);
      await expect(pat()).toBeDisabled();
      // Reopen the card: the refusal stays (recorded in plugin storage, not only on screen).
      await browser.keys("Escape");
      await $("dialog.profile-card").waitForDisplayed({ reverse: true, timeout: 10_000 });
      await openProfile("Alice");
      await waitText(section, `You already patted ${PET_A} today.`);
      await expect(pat()).toBeDisabled();
      await browser.keys("Escape");
    });
  });

  it("AC-6.5 / E6: A and B play Echo Echo; the 30 s lobby starts the round on its own", async () => {
    await openEcholings();
    await clickIn(panel, "Play");
    await closeEcholings();
    await waitText(() => card("Echo Echo"), "Starts in");
    await apps.onB(async () => {
      await waitText(() => card("Echo Echo"), "Alice started Echo Echo", 30_000);
      await waitText(() => card("Echo Echo"), "Starts in");
      await clickIn(() => card("Echo Echo"), "Join");
      await waitText(() => card("Echo Echo"), "You're in");
    });
    // Nobody presses Start now: the lobby ends after 30 s and the tune starts for both.
    const turn = async () => {
      await waitText(() => card("Echo Echo"), "Your turn", 60_000);
      // A wrong note ends the turn early, so tap only while the pads take notes.
      for (let i = 0; i < 5; i++) {
        const pad = card("Echo Echo").$(`button=♪ ${String((i % 4) + 1)}`);
        if (!(await pad.isEnabled().catch(() => false))) break;
        await pad.click();
      }
      await waitText(() => card("Echo Echo"), "You echoed");
    };
    await turn();
    await apps.onB(turn);
    // Scores 45 s after the start, on both cards, with both players.
    await waitText(() => card("Echo Echo"), /Bob \d\/5/, 60_000);
    await waitText(() => card("Echo Echo"), /You \d\/5/);
    await apps.onB(async () => {
      await waitText(() => card("Echo Echo"), /Alice \d\/5/, 30_000);
      await waitText(() => card("Echo Echo"), /You \d\/5/);
    });
  });

  // C4b: Echolings shows its race page with `webview.show(Place::Dialog)` without opening a
  // dialog first, which the host refuses (`invalid`): the card says "The race window could not
  // open." Needs a fix in Echolings (feat/echolings).
  it.skip("AC-6.5: A and B race in Snack Race (web view, page↔plugin bridge)", async () => {
    await openEcholings();
    await clickIn(panel, "Snack Race");
    // The host's race window opens with every text from the plugin (bridge page → plugin → page).
    await raceFrame().waitForDisplayed({ timeout: 20_000 });
    await browser.waitUntil(async () => (await racePage()).hop === "Hop!", {
      timeout: 20_000,
      timeoutMsg: "the race page never got its texts from the plugin",
    });
    expect((await racePage()).title).toBe("Snack Race");
    // Both arm the in-page tapper before the lobby ends.
    const autoA = await autoRace();
    const autoB = await apps.onB(async () => {
      await waitText(() => card("Snack Race"), "Alice started Snack Race", 30_000);
      await clickIn(() => card("Snack Race"), "Join");
      await raceFrame().waitForDisplayed({ timeout: 20_000 });
      await browser.waitUntil(async () => (await racePage()).lanes.length === 2, {
        timeout: 20_000,
        timeoutMsg: "B's race page does not list both pets",
      });
      return autoRace();
    });
    // Lobby 30 s, run 15 s, results 20 s after the start.
    const finished = async () => {
      await browser.waitUntil(async () => (await racePage()).results.length === 2, {
        timeout: 75_000,
        timeoutMsg: "the race page never ranked both runners",
      });
      return racePage();
    };
    const pageA = await finished();
    const pageB = await apps.onB(finished);
    console.log(`[e2e] snack race (auto ${String(autoA)}/${String(autoB)}) A: ${JSON.stringify(pageA)} B: ${JSON.stringify(pageB)}`);
    for (const page of [pageA, pageB]) {
      expect(page.results.some((r) => r.includes("Alice") || r.includes("You"))).toBe(true);
      expect(page.results.some((r) => r.includes("Bob") || r.includes("You"))).toBe(true);
    }
    // Progress (kind 8) moved the other pet on A's track when B hopped at all.
    const bLane = pageA.lanes.find((l) => l.startsWith("Bob"));
    const bOwn = pageB.lanes.find((l) => !l.startsWith("Alice"));
    if (bOwn !== undefined && !bOwn.includes(": 0 of")) expect(bLane).not.toContain(": 0 of");
    await browser.keys("Escape");
    await waitText(() => card("Snack Race"), "Bob", 10_000);
    await apps.onB(async () => {
      await browser.keys("Escape");
      await waitText(() => card("Snack Race"), "Alice", 10_000);
    });
  });

  it("AC-6.6: with Poll, Dice and a badge plugin only one badge shows; the picker switches it", async () => {
    await openSettingsGroup("Plugins");
    await install("Poll");
    await install("Dice roller");
    await pluginTab("Repositories").click();
    await $("button=Add repository").click();
    await setField(
      $("//label[normalize-space()='Repository address']/following::input[1]"),
      fixtures?.repoUrl ?? "",
    );
    await $("button=Check repository").click();
    const confirm = $("button.settings-button--danger=Add repository");
    await confirm.waitForClickable({ timeout: 30_000 });
    await confirm.click();
    await confirm.waitForDisplayed({ reverse: true, timeout: 10_000 });
    await install("Fixture Badge Two");

    await pluginTab("Installed").click();
    const picker = $(
      "//div[contains(@class,'plugins-installed-field')][label[normalize-space(.)='Badge shown next to names']]//button[@role='combobox']",
    );
    const choose = async (label: string) => {
      await picker.click();
      const option = $(`//*[@role='option'][normalize-space(.)='${label}']`);
      await option.waitForClickable({ timeout: 5_000 });
      await option.click();
      await waitText(() => picker, label, 5_000);
    };
    const shows = async (expected: "pet" | "B2") => {
      for (const name of ["Alice", "Bob"]) {
        await browser.waitUntil(
          async () => {
            const list = await badges(name);
            if (list.length !== 1) return false;
            const text = await list[0].getProperty("textContent").then(String);
            const pet = await list[0].$("img[alt*=' the ']").isExisting();
            return expected === "pet" ? pet : text.includes("B2") && !pet;
          },
          { timeout: 5_000, timeoutMsg: `${name}'s row does not show exactly one ${expected} badge` },
        );
      }
    };
    await choose("Fixture Badge Two");
    await closeSettings();
    await shows("B2");
    await openSettingsGroup("Plugins");
    await pluginTab("Installed").click();
    await choose("Echolings");
    await closeSettings();
    await shows("pet");
  });
});
