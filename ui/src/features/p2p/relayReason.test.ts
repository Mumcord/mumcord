import { describe, expect, it } from "vitest";
import type { RelayRefusal, RelayUnavailable } from "../../api";
import { i18n } from "../../i18n";
import { p2pBlockText, relayReasonText } from "./relayReason";

const t = i18n.getFixedT("en", "p2p");

describe("relay reason texts", () => {
  it.each([
    [{ kind: "noRelay" }, "This server has no relay."],
    [{ kind: "notConnected" }, "Not connected to this server."],
    [{ kind: "untrusted" }, "The relay of this server changed and is not trusted."],
    [{ kind: "conflict" }, "Two relays are announced on this server; none is used."],
    [{ kind: "botLeft" }, "The relay bot left the server."],
    [{ kind: "relayDown" }, "The relay is not reachable right now (reported by the relay bot)."],
    [{ kind: "botNoAnswer" }, "The relay bot did not answer."],
    [{ kind: "unreachable" }, "The relay could not be reached over UDP or TCP 443."],
    [
      { kind: "peerCannotRelay" },
      "The other person can't use the relay (older Mumcord or relay turned off).",
    ],
    [{ kind: "denied" }, "The relay bot refused the request."],
    [{ kind: "peerGone" }, "The other person left the server."],
    [{ kind: "rateLimited" }, "Too many relay requests. Try again in a minute."],
    [{ kind: "relayFull" }, "The relay refused the connection: relay full"],
  ] as [RelayUnavailable, string][])("%o reads %s", (reason, text) => {
    expect(relayReasonText(t, reason)).toBe(text);
  });

  it.each([
    "unknownToken",
    "expired",
    "notAllowed",
    "botGone",
    "relayFull",
    "peerLimit",
    "rateLimited",
    "version",
    "badRequest",
  ] as RelayRefusal[])("names the refusal %s", (refusal) => {
    const text = relayReasonText(t, { kind: "refused", refusal });
    expect(text).toMatch(/^The relay refused the connection: [a-z ]+$/);
  });

  it("explains the P2P watch blocks and leaves the others to the caller", () => {
    expect(p2pBlockText(t, "p2pOff", null)).toBe("Peer-to-peer is off.");
    expect(p2pBlockText(t, "relayUnavailable", { kind: "botLeft" })).toBe(
      "The relay bot left the server.",
    );
    expect(p2pBlockText(t, "consentOff", null)).toBeNull();
  });
});
