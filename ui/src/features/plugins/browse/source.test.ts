import { describe, expect, it } from "vitest";
import type { IndexSource, InstalledPlugin } from "../../../api";
import { needsRepositoryWarning } from "./source";

function installed(source: InstalledPlugin["source"]): InstalledPlugin {
  return {
    id: "fan-pets",
    name: "Fan Pets",
    version: "1.0.0",
    source,
    status: { kind: "running" },
    enabled: true,
    disabledReason: null,
    required: [],
    optional: [],
    granted: [],
    visibility: "everyone",
    pinned: false,
    notifications: true,
    installedAt: null,
    badge: false,
    revocation: null,
    icon: null,
    description: "",
    authors: [],
    offered: [],
  };
}

const fanRepo: IndexSource = { kind: "repo", repositoryId: 3, name: "Fan Repo" };

describe("needsRepositoryWarning", () => {
  it("warns only before the first install from each third-party repository", () => {
    expect(needsRepositoryWarning(fanRepo, [])).toBe(true);
    expect(needsRepositoryWarning(fanRepo, [installed({ kind: "official" })])).toBe(true);
    expect(
      needsRepositoryWarning(fanRepo, [
        installed({ kind: "repo", repositoryId: 4, name: "Other Repo" }),
      ]),
    ).toBe(true);
    expect(
      needsRepositoryWarning(fanRepo, [
        installed({ kind: "repo", repositoryId: 3, name: "Fan Repo" }),
      ]),
    ).toBe(false);
  });

  it("never warns for official plugins", () => {
    expect(needsRepositoryWarning({ kind: "official" }, [])).toBe(false);
  });
});
