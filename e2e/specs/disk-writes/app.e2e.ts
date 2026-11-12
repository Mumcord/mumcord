// Spec 12 (reduce SSD writes), the settings part that unit tests cannot cover: the four new
// switches are exposed in the UI and survive a restart (AC-12.4). AC-12.1 (log writes are
// buffered), AC-12.3 (chat history batching) and AC-12.5/12.6 (caches stay unwritten) are
// covered deterministically by the store/engine unit tests; this spec checks the user-visible
// wiring and persistence.
import { expect } from "@wdio/globals";
import { app } from "../../helpers/index.ts";
import { openSettingsGroup, settingsCard } from "../../helpers/settings-ui.ts";
import { wizard } from "../app/flows.ts";

/** The `role="switch"` input of the switch whose label starts with `label`, inside `cardTitle`. */
function switchIn(cardTitle: string, label: string) {
  return settingsCard(cardTitle).$(
    `//div[contains(concat(' ',normalize-space(@class),' '),' settings-switch ')][.//*[starts-with(normalize-space(.),'${label}')]]//input[@role='switch']`,
  );
}

const DIAGNOSTICS = "Diagnostics";
const DISK_WRITES = "Reduce disk writes";
const SWITCHES: readonly { card: string; label: string }[] = [
  { card: DIAGNOSTICS, label: "Write logs to disk" },
  { card: DISK_WRITES, label: "Wait for disk on every save" },
  { card: DISK_WRITES, label: "Keep link previews on disk" },
  { card: DISK_WRITES, label: "Keep plugin registry copy on disk" },
];

describe("disk writes", () => {
  it("shows the four switches, all on by default, and keeps them off after a restart", async () => {
    await wizard("DiskWritesTester");

    await openSettingsGroup("Advanced & Developer");

    // All four switches exist and default to on.
    for (const { card, label } of SWITCHES) {
      const toggle = switchIn(card, label);
      await toggle.waitForExist({ timeout: 10_000 });
      await toggle.scrollIntoView();
      await expect(toggle).toBeSelected();
    }
    // The level dropdown stays next to the disk switch (proposal C).
    await expect(
      settingsCard(DIAGNOSTICS).$("//select | //button[contains(@class,'select__button')]"),
    ).toExist();

    // Turn all four off.
    for (const { card, label } of SWITCHES) {
      const toggle = switchIn(card, label);
      await toggle.scrollIntoView();
      await toggle.click();
      await expect(toggle).not.toBeSelected();
    }

    // Restart with the same profile: every choice survives (AC-12.4).
    await app.restart();
    await openSettingsGroup("Advanced & Developer");
    for (const { card, label } of SWITCHES) {
      const toggle = switchIn(card, label);
      await toggle.waitForExist({ timeout: 10_000 });
      await toggle.scrollIntoView();
      await expect(toggle).not.toBeSelected();
    }
  });
});
