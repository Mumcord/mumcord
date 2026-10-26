import { describe, expect, it } from "vitest";
import { i18n } from "../../i18n";
import { connectionErrorText } from "./errors";

describe("connection error text", () => {
  const t = i18n.getFixedT("en", "servers");

  it("points to the macOS Local Network permission when there is no route", () => {
    expect(connectionErrorText(t, { code: "localNetworkBlocked", message: "" })).toContain(
      "System Settings → Privacy & Security → Local Network",
    );
  });

  it("explains a version refusal instead of a generic error", () => {
    expect(connectionErrorText(t, { code: "serverTooOld", message: "" })).toContain(
      "doesn't accept this version of Mumcord",
    );
  });
});
