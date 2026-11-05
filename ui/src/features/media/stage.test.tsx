/** The stage switcher of the media area (FR-9.44, FR-9.50): tabs, keys, dots and kept items. */
import { act, fireEvent, render, screen } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { MediaArea } from "./MediaArea";
import {
  closeMedia,
  mediaView,
  openMedia,
  resetMediaView,
  selectStage,
  useMediaView,
} from "./mediaView";
import { StagePanel } from "./StagePanel";
import { registerStageItem, resetStageEntries, useStageLabel } from "./stageRegistry";
import { useMediaKeys } from "./useMediaKeys";

/** A built-in view stand-in that counts mounts and unmounts, like a running stream would. */
const lifecycle = { mounted: 0, unmounted: 0 };
function FakeShare({ onMute }: { onMute: () => void }) {
  useStageLabel("share", "Jonas's screen");
  useMediaKeys({ mute: onMute });
  useEffect(() => {
    lifecycle.mounted += 1;
    return () => {
      lifecycle.unmounted += 1;
    };
  }, []);
  return <p>share picture</p>;
}

function Area({ onMute = () => undefined }: { onMute?: () => void }) {
  const view = useMediaView();
  return (
    <MediaArea>
      {view.stage.includes("share") && (
        <StagePanel id="share">
          <FakeShare onMute={onMute} />
        </StagePanel>
      )}
    </MediaArea>
  );
}

function brawl() {
  return registerStageItem("plugin:board-brawl", {
    label: "Board Brawl",
    render: () => <p>board</p>,
  });
}

beforeEach(async () => {
  await applyLanguage("en");
  resetMediaView();
  resetStageEntries();
  lifecycle.mounted = 0;
  lifecycle.unmounted = 0;
});

afterEach(() => {
  resetMediaView();
  resetStageEntries();
});

describe("stage store", () => {
  it("opens the first item, adds later ones as tabs with a dot", () => {
    openMedia("share");
    const unregister = brawl();
    expect(mediaView()).toMatchObject({
      cinema: "share",
      stage: ["share", "plugin:board-brawl"],
      unseen: ["plugin:board-brawl"],
    });
    selectStage("plugin:board-brawl");
    expect(mediaView()).toMatchObject({ cinema: "plugin:board-brawl", unseen: [] });
    unregister();
    expect(mediaView()).toMatchObject({ cinema: "share", stage: ["share"] });
  });

  it("updating a registered activity neither re-adds nor selects it", () => {
    openMedia("share");
    brawl();
    const update = registerStageItem("plugin:board-brawl", { label: "Brawl", render: () => null });
    expect(mediaView()).toMatchObject({ cinema: "share", stage: ["share", "plugin:board-brawl"] });
    update();
    expect(mediaView().stage).toEqual(["share"]);
  });

  it("closing the main item hands the stage to its neighbour", () => {
    openMedia("watch");
    openMedia("share");
    selectStage("share");
    closeMedia("share");
    expect(mediaView().cinema).toBe("watch");
  });
});

describe("stage tabs", () => {
  it("shows no tab row for a single item", () => {
    openMedia("share");
    render(<Area />);
    expect(screen.queryByRole("tablist")).toBeNull();
    expect(screen.getByText("share picture")).toBeVisible();
  });

  it("adds a new item as a tab with a dot that does not take the stage", async () => {
    openMedia("share");
    const { container } = render(<Area />);
    act(() => {
      brawl();
    });
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((tab) => tab.getAttribute("aria-label"))).toEqual([
      "Jonas's screen, tab 1 of 2",
      "Board Brawl, plugin activity, new, tab 2 of 2",
    ]);
    expect(tabs[0]).toHaveAttribute("aria-selected", "true");
    expect(container.querySelector(".media-stage__dot")).not.toBeNull();
    expect(screen.getByText("share picture")).toBeVisible();
    expect(screen.getByText("board")).not.toBeVisible();
    expect(screen.getByText(/Switching never stops anything · Alt\+1 \/ Alt\+2/)).toBeVisible();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("switches with Alt+digit and arrow keys and keeps every item mounted", () => {
    openMedia("share");
    render(<Area />);
    act(() => {
      brawl();
    });
    fireEvent.keyDown(window, { key: "2", code: "Digit2", altKey: true });
    expect(mediaView().cinema).toBe("plugin:board-brawl");
    expect(screen.getByText("board")).toBeVisible();
    expect(screen.getByText("share picture")).not.toBeVisible();
    const selected = screen.getByRole("tab", { selected: true });
    expect(selected).toHaveAttribute("aria-label", "Board Brawl, plugin activity, tab 2 of 2");
    expect(selected).toHaveAttribute("tabindex", "0");

    fireEvent.keyDown(selected, { key: "ArrowRight" });
    expect(mediaView().cinema).toBe("share");
    expect(screen.getByRole("tab", { selected: true })).toHaveFocus();
    fireEvent.keyDown(screen.getByRole("tab", { selected: true }), { key: "End" });
    expect(mediaView().cinema).toBe("plugin:board-brawl");
    fireEvent.click(screen.getByRole("tab", { name: /Jonas's screen/ }));
    expect(mediaView().cinema).toBe("share");

    // The share was never stopped by the switches.
    expect(lifecycle).toEqual({ mounted: 1, unmounted: 0 });
  });

  it("switches within 200 ms", () => {
    openMedia("share");
    render(<Area />);
    act(() => {
      brawl();
    });
    const start = performance.now();
    fireEvent.keyDown(window, { key: "2", code: "Digit2", altKey: true });
    expect(screen.getByText("board")).toBeVisible();
    expect(performance.now() - start).toBeLessThan(200);
  });

  it("leaves the media keys to the main item", () => {
    const mute = vi.fn();
    openMedia("share");
    render(<Area onMute={mute} />);
    fireEvent.keyDown(document.body, { key: "m" });
    expect(mute).toHaveBeenCalledTimes(1);
    act(() => {
      brawl();
      selectStage("plugin:board-brawl");
    });
    fireEvent.keyDown(document.body, { key: "m" });
    expect(mute).toHaveBeenCalledTimes(1);
  });
});
