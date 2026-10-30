import { describe, expect, it } from "vitest";
import { fullFingerprint, identityInitial, identityTone, shortFingerprint } from "./identityFormat";

const HASH = "4fa29c1be73055d8c1c1f0a1b2c3d4e5f6071829";

describe("identity fingerprints", () => {
  it("groups every byte pair, in upper case", () => {
    expect(fullFingerprint("4fa29c1b")).toBe("4F:A2:9C:1B");
    expect(fullFingerprint(HASH).split(":")).toHaveLength(20);
  });

  it("shortens long hashes to the first eight and last two pairs", () => {
    expect(shortFingerprint(HASH)).toBe("4F:A2:9C:1B:E7:30:55:D8:…:18:29");
    expect(shortFingerprint("4fa29c1b")).toBe("4F:A2:9C:1B");
  });

  it("derives the avatar letter and a stable colour from the identity", () => {
    expect(identityInitial("Streaming")).toBe("S");
    expect(identityInitial("  zoe ")).toBe("Z");
    expect(identityInitial("")).toBe("?");
    expect(identityTone(7)).toBe(identityTone(13));
    expect(identityTone(7)).toMatch(/^identity-card__avatar--tone\d$/);
  });
});
