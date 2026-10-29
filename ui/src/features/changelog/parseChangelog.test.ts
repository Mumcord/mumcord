import { describe, expect, it } from "vitest";
import changelog from "../../../../CHANGELOG.md?raw";
import { parseChangelog } from "./parseChangelog";

describe("parseChangelog", () => {
  it("reads every heading style of CHANGELOG.md", () => {
    const versions = parseChangelog(
      [
        "# Changelog",
        "",
        "## [Unreleased]",
        "",
        "### Added",
        "",
        "- Next thing",
        "",
        "## [0.4.4] - Unreleased",
        "",
        "### Fixed",
        "",
        "- A fix (issue #135)",
        "",
        "## [0.4.3] - 2026-10-28",
        "",
        "### Changed",
        "",
        "- A change",
        "",
        "## 0.1.0 – 2026-10-25",
        "",
        "- An item before any group",
      ].join("\n"),
    );
    expect(versions.map((v) => [v.version, v.date, v.unreleased])).toEqual([
      ["Unreleased", null, true],
      ["0.4.4", null, true],
      ["0.4.3", "2026-10-28", false],
      ["0.1.0", "2026-10-25", false],
    ]);
    expect(versions[1]?.sections).toEqual([{ title: "Fixed", items: ["A fix (issue #135)"] }]);
    expect(versions[3]?.sections).toEqual([{ title: "", items: ["An item before any group"] }]);
  });

  it("keeps the summary paragraph, the release links and Markdown as written", () => {
    const [version] = parseChangelog(
      [
        "## [0.4.0] - 2026-10-26",
        "",
        "_Watch party is **Beta**._",
        "",
        "### Multi-server",
        "",
        "- Open links ([`745fe60`](https://github.com/Mumcord/mumcord/commit/745fe60))",
        "",
        "[0.4.0]: https://github.com/Mumcord/mumcord/releases/tag/v0.4.0",
      ].join("\n"),
    );
    expect(version).toEqual({
      version: "0.4.0",
      date: "2026-10-26",
      unreleased: false,
      summary: "_Watch party is **Beta**._",
      url: "https://github.com/Mumcord/mumcord/releases/tag/v0.4.0",
      sections: [
        {
          title: "Multi-server",
          items: ["Open links ([`745fe60`](https://github.com/Mumcord/mumcord/commit/745fe60))"],
        },
      ],
    });
  });

  it("joins wrapped and nested lines to their item and copes with malformed input", () => {
    const versions = parseChangelog(
      [
        "### Before any version",
        "- ignored",
        "## not a version heading at all",
        "- ignored as well",
        "## [1.0.0] - someday",
        "### Added",
        "* Star item",
        "  continues here",
        "  - nested",
        "-",
        "A loose paragraph",
        "### Empty",
        "",
        "## [ ] broken",
      ].join("\r\n"),
    );
    expect(versions).toEqual([
      {
        version: "1.0.0",
        date: null,
        unreleased: false,
        sections: [
          {
            title: "Added",
            items: ["Star item\ncontinues here\n- nested", "-", "A loose paragraph"],
          },
        ],
      },
    ]);
    expect(parseChangelog("")).toEqual([]);
  });

  it("parses the real CHANGELOG.md", () => {
    const versions = parseChangelog(changelog);
    expect(versions.length).toBeGreaterThan(5);
    expect(versions.at(-1)).toMatchObject({ version: "0.1.0", summary: "_First release._" });
    for (const version of versions) {
      expect(version.version).toMatch(/^(?:\d+\.\d+\.\d+|Unreleased)$/u);
      expect(version.unreleased || /^\d{4}-\d{2}-\d{2}$/u.test(version.date ?? "")).toBe(true);
      for (const section of version.sections) expect(section.items.length).toBeGreaterThan(0);
    }
  });
});
