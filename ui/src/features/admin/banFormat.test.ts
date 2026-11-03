import { describe, expect, it } from "vitest";
import type { BanEntry } from "../../api";
import {
  endTime,
  filterBans,
  looksLikeAddress,
  maskAddress,
  searchBans,
  shortHash,
  sortBans,
} from "./banFormat";

const mallory: BanEntry = {
  key: "203.0.113.1/32|a3f9c21e8b4d6f2a1c5e7b9d0f1a2b3c4d5e6f70",
  name: "Mallory",
  certHash: "a3f9c21e8b4d6f2a1c5e7b9d0f1a2b3c4d5e6f70",
  address: "203.0.113.1",
  mask: 32,
  reason: "spam links",
  start: "2026-11-03T20:21:00.000Z",
  durationSecs: 604_800,
  expired: false,
};
const nobody: BanEntry = {
  key: "198.51.100.9/32|",
  name: null,
  certHash: null,
  address: "198.51.100.9",
  mask: 32,
  reason: "flooding",
  start: "2026-10-01T09:00:00.000Z",
  durationSecs: 0,
  expired: false,
};
const old: BanEntry = {
  ...mallory,
  key: "2001:db8:0:1:0:0:0:5/64|b7c2d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3",
  certHash: "b7c2d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3",
  address: "2001:db8:0:1:0:0:0:5",
  mask: 64,
  reason: "already served",
  start: "2020-01-01T00:00:00.000Z",
  durationSecs: 3600,
  expired: true,
};

describe("ban list display", () => {
  it("shortens a fingerprint and masks the personal part of an address", () => {
    expect(shortHash(mallory.certHash)).toBe("a3f9…6f70");
    expect(maskAddress(mallory.address, mallory.mask)).toBe("203.0.113.*");
    expect(maskAddress("203.0.113.0", 24)).toBe("203.0.*");
    expect(maskAddress(nobody.address, 32)).toBe("198.51.100.*");
    expect(maskAddress(old.address, 64)).toBe("2001:db8:0:1:*");
  });

  it("reads the end of a ban and knows permanent and expired", () => {
    expect(endTime(mallory)).not.toBeNull();
    expect(endTime(nobody)).toBeNull();
    expect(filterBans([mallory, old], "expired")).toEqual([old]);
    expect(filterBans([mallory, old], "active")).toEqual([mallory]);
    expect(filterBans([mallory, old], "all")).toHaveLength(2);
  });

  it("searches name, address, fingerprint and reason (FR-10.53)", () => {
    expect(searchBans([mallory, nobody, old], "spam")).toEqual([mallory]);
    expect(searchBans([mallory, nobody, old], "198.51")).toEqual([nobody]);
    expect(searchBans([mallory, nobody, old], "a3f9")).toEqual([mallory]);
    expect(searchBans([mallory, nobody, old], "")).toHaveLength(3);
  });

  it("sorts by one column", () => {
    const sorted = sortBans([nobody, mallory, old], { column: "ends", ascending: true });
    expect(sorted[0]).toBe(old);
    expect(sorted[1]).toBe(mallory);
    expect(sorted[2]).toBe(nobody); // permanent last
    expect(sortBans([mallory, old], { column: "name", ascending: false })[0]).toBe(mallory);
  });

  it("accepts only addresses it can send back", () => {
    expect(looksLikeAddress("203.0.113.1")).toBe(true);
    expect(looksLikeAddress("2001:db8::1")).toBe(true);
    expect(looksLikeAddress("mallory.example")).toBe(false);
    expect(looksLikeAddress("300.1.1.1")).toBe(false);
  });
});
