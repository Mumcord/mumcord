// UI steps shared by the voice specs: first-start wizard, connecting, reading the channel tree.
import { browser, $ } from "@wdio/globals";

export const SERVER = "localhost";

/** Wizard on a fresh profile: new identity, default devices, voice activation. */
export async function completeWizard(name: string): Promise<void> {
  const nameInput = $("input[autocomplete='nickname']");
  await nameInput.waitForDisplayed({ timeout: 30_000 });
  await nameInput.setValue(name);
  await $("button=Next").click();
  await $("h2=Microphone").waitForDisplayed({ timeout: 30_000 });
  await $("button=Next").click();
  await $("h2=How do you want to talk?").waitForDisplayed();
  // Keep voice activation; don't touch the system's mumble:// handler from a test.
  const link = $("//label[contains(., 'Open mumble:// links')]/input");
  if ((await link.isExisting()) && (await link.isSelected())) await link.click();
  await $("button=Finish").click();
}

/** Adds the server from the empty state and trusts its certificate. */
export async function addAndConnect(address = `${SERVER}:64738`): Promise<void> {
  const input = $("//label[text()='Server address or mumble:// link']/following::input[1]");
  await input.waitForDisplayed({ timeout: 30_000 });
  await input.setValue(address);
  await $("button=Add and connect").click();
  const trust = $("button=Trust and connect");
  await trust.waitForDisplayed({ timeout: 20_000 });
  await trust.click();
  await waitConnected();
}

export async function waitConnected(): Promise<void> {
  await $(`button[aria-label="${SERVER} (connected)"]`).waitForExist({ timeout: 30_000 });
  await $("[role='tree']").waitForExist({ timeout: 15_000 });
}

/** Connects to the saved server from the server bar (after a restart). */
export async function reconnect(): Promise<void> {
  const button = $(`button[aria-label="${SERVER}"]`);
  await button.waitForDisplayed({ timeout: 30_000 });
  await button.click();
  await waitConnected();
  await joinIfReadOnly();
}

/** Name of the channel the own user is in (from the rendered tree). */
export async function ownChannel(): Promise<string> {
  return browser.execute(() => {
    const you = document.querySelector("[role='tree'] .tree-person__you");
    const group = you?.closest("[role='group']");
    const item = group?.id ? document.querySelector(`[aria-owns="${group.id}"]`) : null;
    const host = item ?? group?.parentElement?.closest(".tree-channel") ?? null;
    return host?.querySelector(".tree-channel__name")?.textContent ?? "";
  });
}

/**
 * A fresh connect may land read-only (self muted + deafened until an explicit join). Then join
 * the current channel explicitly (double-click); with the older behaviour this is a no-op.
 */
export async function joinIfReadOnly(): Promise<void> {
  const deafen = $("button[aria-label='Deafen']");
  await deafen.waitForExist({ timeout: 15_000 });
  // Give the server's initial state a moment to arrive.
  await browser.pause(500);
  if ((await deafen.getAttribute("aria-pressed")) !== "true") return;
  await browser.waitUntil(async () => (await ownChannel()) !== "", { timeout: 15_000 });
  await channelName(await ownChannel()).doubleClick();
  await browser.waitUntil(async () => (await deafen.getAttribute("aria-pressed")) !== "true", {
    timeout: 15_000,
    timeoutMsg: "still deafened after explicit join",
  });
  const mute = $("button[aria-label='Mute microphone']");
  await browser.waitUntil(async () => (await mute.getAttribute("aria-pressed")) !== "true", {
    timeout: 15_000,
    timeoutMsg: "still muted after explicit join",
  });
}

/** Channel name span (click target) of a channel row. */
export function channelName(name: string) {
  return $(
    `//div[@role='treeitem' and contains(@class,'tree-channel')]/div[contains(@class,'tree-channel__row')]//span[contains(@class,'tree-channel__name') and text()='${name}']`,
  );
}

/** Person row in the tree (members, not listeners) by exact name. */
export function personRow(name: string) {
  return $(
    `//div[contains(@class,'tree-person') and @data-listener='false'][.//span[contains(@class,'tree-person__name') and text()='${name}']]`,
  );
}

export interface TreeChannel {
  name: string;
  level: number;
  people: string[];
}

/** The rendered tree: every channel with the people directly in it. */
export async function readTree(): Promise<TreeChannel[]> {
  return browser.execute(() => {
    const out: { name: string; level: number; people: string[] }[] = [];
    document.querySelectorAll<HTMLElement>("[role='tree'] .tree-channel").forEach((ch) => {
      const name = ch.querySelector(":scope > .tree-channel__row .tree-channel__name")?.textContent;
      // The group is either nested or a sibling linked via aria-owns (flat ARIA tree).
      const owns = ch.getAttribute("aria-owns");
      const group =
        ch.querySelector(":scope > [role='group']") ??
        (owns ? document.getElementById(owns) : null);
      const people = group
        ? Array.from(group.querySelectorAll(":scope > .tree-person[data-listener='false']")).map(
            (p) => p.querySelector(".tree-person__name")?.firstChild?.textContent ?? "",
          )
        : [];
      out.push({ name: name ?? "", level: Number(ch.getAttribute("aria-level")), people });
    });
    return out;
  });
}

/** Records (in the page) the wall-clock time each person's talking indicator turns on. */
export async function watchTalking(): Promise<void> {
  await browser.execute(() => {
    const w = window as unknown as {
      __talkOn: Record<string, number[]>;
      __talkObs?: MutationObserver;
    };
    w.__talkObs?.disconnect();
    w.__talkOn = {};
    const active = new Set<string>();
    const check = () => {
      const now = Date.now();
      const on = new Set<string>();
      document.querySelectorAll(".tree-person[data-talking='true']").forEach((p) => {
        on.add(p.querySelector(".tree-person__name")?.firstChild?.textContent ?? "");
      });
      on.forEach((n) => {
        if (!active.has(n)) (w.__talkOn[n] ??= []).push(now);
      });
      active.clear();
      on.forEach((n) => active.add(n));
    };
    w.__talkObs = new MutationObserver(check);
    w.__talkObs.observe(document.body, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ["data-talking"],
    });
    check();
  });
}

export async function talkingOnTimes(name: string): Promise<number[]> {
  return browser.execute(
    (n: string) => (window as unknown as { __talkOn: Record<string, number[]> }).__talkOn[n] ?? [],
    name,
  );
}
