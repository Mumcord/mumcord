import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PluginScore, PluginScoreFactor } from "../../../bindings/commands";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { biggestReasons, breakdownRows } from "./breakdownRows";
import { ScoreBreakdown, ScoreChip, ScoreMeter, ScoreReasons } from "./index";

beforeEach(async () => {
  await applyLanguage("en");
});

const pts = (value: number) => ({ kind: "points" as const, value });
const pct = (value: number) => ({ kind: "percent" as const, value });

/** Overlay Pals from #393 / mockup 02, as the engine reports it. */
const overlayFactors: PluginScoreFactor[] = [
  { kind: "highPermission", effect: pts(20), detailKey: "net.origins" },
  { kind: "relay", effect: pts(8), detailKey: "peers.relay" },
  { kind: "mediumPermission", effect: pts(6), detailKey: "chat.send" },
  { kind: "mediumPermission", effect: pts(6), detailKey: "events.voice" },
  { kind: "webSurface", effect: pts(10), detailKey: "ui.stage" },
  { kind: "websites", effect: pts(6), detailKey: "origins" },
  { kind: "packageSize", effect: pts(5), detailKey: "package_size" },
  { kind: "wasmModules", effect: pts(2), detailKey: "wasm_modules" },
  { kind: "sourceLink", effect: pct(-5), detailKey: "source_link.given" },
  { kind: "licence", effect: pct(10), detailKey: "licence.not_declared" },
  { kind: "reproducible", effect: pct(0), detailKey: "reproducible.none" },
  { kind: "keyAge", effect: pct(10), detailKey: "key_age.new" },
];

const overlay: PluginScore = {
  value: 72,
  band: "high",
  e: 63,
  f: 1.15,
  h: 0,
  factors: overlayFactors,
  version: 1,
};

describe("ScoreChip", () => {
  it("shows band and number, and reads one complete sentence", async () => {
    const { container } = render(<ScoreChip scored={{ kind: "scored", score: overlay }} />);
    expect(container.textContent).toContain("High risk");
    expect(container.textContent).toContain("· 72");
    expect(
      screen.getByText("Security score 72 out of 100. High risk. Higher means more risk."),
    ).toBeInTheDocument();
    expect(container.querySelector(".risk-chip")?.getAttribute("data-level")).toBe("high");
    expect(container.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("labels official plugins without a number", async () => {
    const { container } = render(<ScoreChip scored={{ kind: "official" }} />);
    expect(container.textContent).toContain("Official · reviewed");
    expect(container.textContent).not.toMatch(/\d/);
    expect(screen.getByText("Official, reviewed.")).toHaveClass("visually-hidden");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows hard stops and blocked plugins without a number", () => {
    const { container, rerender } = render(
      <ScoreChip scored={{ kind: "refused", stop: "unsafeOrigin" }} />,
    );
    expect(container.textContent).toContain("Can't be installed");
    expect(container.textContent).not.toMatch(/\d/);
    rerender(<ScoreChip scored={{ kind: "scored", score: overlay }} blocked />);
    expect(container.textContent).toContain("Blocked by your setting");
    expect(container.textContent).toContain("Blocked by your safety setting");
    expect(container.textContent).not.toMatch(/\d/);
  });

  it("marks developer plugins and reads the developer sentence", async () => {
    const low: PluginScore = { ...overlay, value: 8, band: "low" };
    const { container } = render(<ScoreChip scored={{ kind: "scored", score: low }} developer />);
    expect(container.textContent).toContain("Developer plugin");
    expect(screen.getByText("Low risk. Security score 8. Developer plugin.")).toHaveClass(
      "visually-hidden",
    );
    expect(await axeViolations(container)).toEqual([]);
  });
});

describe("ScoreMeter", () => {
  it("is a meter with value and spoken value", async () => {
    const { container } = render(<ScoreMeter value={72} band="high" />);
    const meter = screen.getByRole("meter", { name: "Security score" });
    expect(meter).toHaveAttribute("aria-valuenow", "72");
    expect(meter).toHaveAttribute("aria-valuemin", "0");
    expect(meter).toHaveAttribute("aria-valuemax", "100");
    expect(meter).toHaveAttribute("aria-valuetext", "72 out of 100, high risk");
    expect(container.querySelector<HTMLElement>(".risk-meter__fill")?.style.width).toBe("72%");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("takes its own name", () => {
    render(<ScoreMeter value={37} band="medium" label="Score now" />);
    expect(screen.getByRole("meter", { name: "Score now" })).toHaveAttribute(
      "aria-valuetext",
      "37 out of 100, medium risk",
    );
  });
});

describe("ScoreBreakdown", () => {
  it("is three real tables with captions and headers, effects read in words", async () => {
    const { container } = render(
      <ScoreBreakdown
        score={overlay}
        pluginName="Overlay Pals"
        facts={{ websites: 2, packageSize: "12 MiB", wasmModules: 2, keyAgeDays: 10 }}
      />,
    );
    expect(
      screen.getByRole("group", { name: "How the security score of Overlay Pals is made up" }),
    );
    const tables = screen.getAllByRole("table");
    expect(tables.map((t) => t.querySelector("caption")?.textContent)).toEqual([
      "1 · What the plugin is able to do",
      "2 · How much Mumcord can verify (changes the points above)",
      "3 · Recent changes between versions",
    ]);
    const capabilities = tables[0] as HTMLElement;
    expect(
      within(capabilities)
        .getAllByRole("columnheader")
        .map((h) => h.textContent),
    ).toEqual(["Factor", "What Mumcord found", "Points"]);
    // Web surfaces first, then by weight class, as in mockup 02.
    expect(
      within(capabilities)
        .getAllByRole("rowheader")
        .map((h) => h.textContent),
    ).toEqual([
      "Show an activity in the media area",
      "Talk to these websites",
      "Send data through the server's relay",
      "Post a chat message for you after you click or type a command",
      "Know who is talking",
      "More than one website",
      "Package size and code modules",
    ]);
    const stage = within(capabilities).getByRole("row", { name: /media area/ });
    expect(stage.textContent).toContain("ui.stage · first web surface");
    expect(within(stage).getByText("plus 10 points, adds risk")).toHaveClass("visually-hidden");
    expect(
      within(capabilities).getByRole("row", { name: /Talk to these websites/ }).textContent,
    ).toContain("net.origins · 2 websites");
    // Package size and module count share one row, with the engine's points shown together.
    const pkg = within(capabilities).getByRole("row", { name: /Package size/ });
    expect(pkg.textContent).toContain("12 MiB · 2 WASM modules");
    expect(pkg.textContent).toContain("+7");

    const trust = tables[1] as HTMLElement;
    expect(within(trust).getByRole("row", { name: /Source code link/ }).textContent).toContain(
      "minus 5 percent, reduces risk",
    );
    expect(within(trust).getByRole("row", { name: /Reproducible build/ }).textContent).toContain(
      "no change",
    );
    expect(within(trust).getByRole("row", { name: /Author key/ }).textContent).toContain("10 days");
    expect(tables[2]?.textContent).toContain("None in the last 5 versions");

    expect(container.querySelector(".risk-formula")?.textContent).toBe(
      "Score = 63 points × 1.15 (+15 %) + 0 = 72. Formula version 1",
    );
    expect(await axeViolations(container)).toEqual([]);
  });

  it("explains the network floor and says when there is no history", () => {
    const mood: PluginScore = {
      value: 30,
      band: "medium",
      e: 25,
      f: 1,
      h: 0,
      version: 1,
      factors: [
        { kind: "highPermission", effect: pts(20), detailKey: "net.origins" },
        { kind: "noHistory", effect: pts(0), detailKey: "no_history" },
        { kind: "networkFloor", effect: pts(5), detailKey: "network_floor" },
      ],
    };
    const { container } = render(<ScoreBreakdown score={mood} pluginName="Mood Lights" />);
    expect(container.querySelector(".risk-formula")?.textContent).toBe(
      "Score = 25 points × 1.00 (0 %) + 0 = 25. Raised to 30 because the plugin can reach the internet. Formula version 1",
    );
    expect(screen.getAllByRole("table")[2]?.textContent).toContain("No history available");
  });

  it("is open for medium and high, closed for low, and offers the formula link", () => {
    const onHow = vi.fn();
    const { container, rerender } = render(
      <ScoreBreakdown score={overlay} pluginName="X" collapsible onHowCalculated={onHow} />,
    );
    expect(container.querySelector("details")?.open).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "How the score is calculated" }));
    expect(onHow).toHaveBeenCalledOnce();
    rerender(
      <ScoreBreakdown score={{ ...overlay, value: 2, band: "low" }} pluginName="X" collapsible />,
    );
    expect(container.querySelector("details")?.open).toBe(false);
    expect(screen.getByText("Why this score").tagName).toBe("SUMMARY");
  });

  it("shows unknown detail keys as they are", () => {
    render(
      <ScoreBreakdown
        score={{
          ...overlay,
          factors: [{ kind: "licence", effect: pct(5), detailKey: "licence.x" }],
        }}
        pluginName="X"
      />,
    );
    expect(screen.getByRole("rowheader", { name: "licence.x" })).toBeInTheDocument();
  });
});

describe("ScoreReasons", () => {
  it("lists the three biggest points and the biggest trust penalty (mockup 03)", async () => {
    const { container } = render(<ScoreReasons factors={overlayFactors} />);
    const items = within(screen.getByRole("list", { name: "Biggest reasons" })).getAllByRole(
      "listitem",
    );
    expect(items.map((li) => li.textContent)).toEqual([
      "Talk to these websites+20",
      "Show an activity in the media area+10",
      "Send data through the server's relay+8",
      "Licence: worse than average+10 %",
    ]);
    expect(await axeViolations(container)).toEqual([]);
  });

  it("keeps the engine's order for ties and skips factors that lower the risk", () => {
    const reasons = biggestReasons(
      breakdownRows([
        { kind: "mediumPermission", effect: pts(6), detailKey: "chat.send" },
        { kind: "mediumPermission", effect: pts(6), detailKey: "events.voice" },
        { kind: "sourceLink", effect: pct(-5), detailKey: "source_link.given" },
      ]),
    );
    expect(reasons.map((r) => r.name)).toEqual([
      { kind: "permission", id: "chat.send" },
      { kind: "permission", id: "events.voice" },
    ]);
  });

  it("renders nothing without reasons", () => {
    const { container } = render(<ScoreReasons factors={[]} />);
    expect(container).toBeEmptyDOMElement();
  });
});
