/** The sharer's viewer list and the viewer's quality menu (FR-7.51, FR-7.52, mockups 03, 04). */
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ScreenShare, ShareQuality, ViewerQuality as ViewerFigures } from "../../api";
import "../../i18n";
import { axeViolations } from "../../test/axe";
import { ViewerQualityMenu } from "./ViewerQuality";
import { ViewerList } from "./Viewers";

function viewer(session: number, patch: Partial<ViewerFigures> = {}): ViewerFigures {
  return {
    session,
    fps: 60,
    struggling: false,
    slow: false,
    codec: "av1",
    path: "relay",
    ...patch,
  };
}

function quality(viewers: ViewerFigures[]): ShareQuality {
  return {
    quality: "1440p60",
    lowered: null,
    switched: null,
    width: 2560,
    height: 1440,
    fps: 60,
    kbps: 6000,
    encoder: { codec: "av1", gpu: "nvidia" },
    viewers,
  };
}

function own(viewers: number[]): ScreenShare {
  return {
    session: 1,
    paused: false,
    sound: false,
    soundAvailable: false,
    viewers,
    blocked: null,
    relayReason: null,
    kind: "window",
    elapsedMs: 0,
  };
}

const names = ["", "Me", "Jonas", "Maya", "Ben", "Leo", "Sam", "Zoe", "Kim", "Ola"];

function list(viewers: number[], figures: ShareQuality | null) {
  return render(
    <ViewerList
      own={own(viewers)}
      quality={figures}
      nameOf={(session) => names[session] ?? "Someone"}
      srcOf={() => null}
    />,
  );
}

describe("viewer list", () => {
  it("shows quality, codec, path and the slow badge per viewer", async () => {
    const { container } = list(
      [2, 3, 4],
      quality([
        viewer(2),
        viewer(3, { path: "direct", codec: "vp9" }),
        viewer(4, { path: "broadcast", fps: 30, slow: true }),
      ]),
    );
    expect(screen.getByText("Jonas").closest("li")).toHaveTextContent("1440p · 60 fps · AV1");
    expect(screen.getByText("Jonas").closest("li")).toHaveTextContent("Relay");
    expect(screen.getByText("Maya").closest("li")).toHaveTextContent("1440p · 60 fps · VP9");
    expect(screen.getByText("Maya").closest("li")).toHaveTextContent("Direct");
    const ben = screen.getByText("Ben").closest("li");
    expect(ben).toHaveTextContent("1440p · 30 fps · AV1");
    expect(ben).toHaveTextContent("Relay broadcast");
    expect(ben).toHaveTextContent("Slow connection");
    expect(screen.getByText("3 of 8")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("says whether the share is sent once or as one copy per viewer", () => {
    const { unmount } = list([2], quality([viewer(2)]));
    expect(screen.getByText(/One upload, passed on by the server's relay/)).toBeInTheDocument();
    unmount();
    list([2], quality([viewer(2, { path: "direct" })]));
    expect(screen.getByText(/One copy per viewer/)).toBeInTheDocument();
  });

  it("tags a row's path only when the viewers' paths differ", () => {
    const { unmount } = list([2, 3], quality([viewer(2), viewer(3)]));
    expect(screen.getAllByRole("listitem")[0]).not.toHaveTextContent("Relay");
    unmount();
    list([2, 3], quality([viewer(2), viewer(3, { path: "direct" })]));
    expect(screen.getAllByRole("listitem")[1]).toHaveTextContent("Direct");
  });

  it("takes the viewer limit from the share's path", () => {
    render(
      <ViewerList
        own={own([2, 3])}
        quality={quality([viewer(2), viewer(3)])}
        nameOf={(session) => names[session] ?? "Someone"}
        srcOf={() => null}
        limit={20}
      />,
    );
    expect(screen.getByText("2 of 20")).toBeInTheDocument();
  });

  it("folds the viewers beyond six into 'and N more'", () => {
    const sessions = [2, 3, 4, 5, 6, 7, 8, 9];
    list(sessions, quality(sessions.map((session) => viewer(session))));
    expect(screen.getAllByRole("listitem")).toHaveLength(6);
    expect(screen.getByText("and 2 more")).toBeInTheDocument();
  });

  it("lists names alone while the figures are unknown", () => {
    list([2], null);
    expect(screen.getByText("Jonas").closest("li")).not.toHaveTextContent("fps");
  });
});

describe("viewer quality menu", () => {
  function menu(maxFps: null | 30 | 15, onMaxFps = vi.fn()) {
    const view = render(
      <ViewerQualityMenu
        name="Priya"
        summary="1440p · 60 fps · AV1"
        maxFps={maxFps}
        onMaxFps={onMaxFps}
      />,
    );
    return { ...view, onMaxFps };
  }

  it("names what the sharer sends and offers Auto, up to 30 and up to 15", async () => {
    const { container, onMaxFps } = menu(null);
    fireEvent.click(screen.getByRole("button", { name: "Quality" }));
    expect(screen.getByText(/Set by Priya:/)).toHaveTextContent("1440p · 60 fps · AV1");
    expect(screen.getByRole("button", { name: "Auto" })).toHaveAttribute("aria-pressed", "true");
    expect(await axeViolations(container)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Up to 15" }));
    expect(onMaxFps).toHaveBeenCalledWith(15);
    fireEvent.click(screen.getByRole("button", { name: "Up to 30" }));
    expect(onMaxFps).toHaveBeenLastCalledWith(30);
  });

  it("marks the chosen cap and closes on Escape, returning focus to the button", () => {
    menu(30);
    const button = screen.getByRole("button", { name: "Quality" });
    fireEvent.click(button);
    expect(screen.getByRole("button", { name: "Up to 30" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("group", { name: "Quality" })).toBeNull();
    expect(button).toHaveFocus();
  });
});
