import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi } from "../../api";
import { PreferencesProvider } from "../../app/PreferencesProvider";
import { fakeView } from "../../app/testView";
import { useView, ViewContext } from "../../app/view";
import { ViewProvider } from "../../app/ViewProvider";
import { applyLanguage } from "../../i18n";
import { subscribeAnnouncements } from "../../a11y/announcer";
import { axeViolations } from "../../test/axe";
import { TipsCard } from "../settings/sections/AboutSection";
import { TipsHost } from "./TipsHost";
import { GAP } from "./placement";
import { closeTour, offerTour, startTour, TIPS } from "./tour";

const bubble = () => screen.getByRole("region", { name: "Aaron, the tips helper" });
const button = (name: string) => screen.getByRole("button", { name });
const aaron = () => screen.getByTestId("aaron");

/** A marked tour target at the given place; jsdom has no layout of its own. */
function Target({ id, box }: { id: string; box: Omit<DOMRect, "toJSON" | "x" | "y"> }) {
  return (
    <div
      data-tour={id}
      ref={(element) => {
        if (element) {
          vi.spyOn(element, "getBoundingClientRect").mockReturnValue({
            ...box,
            x: box.left,
            y: box.top,
            toJSON: () => box,
          });
        }
      }}
    />
  );
}

function boxOf(left: number, top: number, width: number, height: number) {
  return { left, top, width, height, right: left + width, bottom: top + height };
}

beforeAll(async () => {
  await applyLanguage("en");
});

afterEach(() => {
  act(() => {
    closeTour();
  });
  setApi(undefined);
  vi.restoreAllMocks();
});

describe("Aaron's tips", () => {
  it("shows nothing until the tour is offered or started", () => {
    render(<TipsHost />);
    expect(
      screen.queryByRole("region", { name: "Aaron, the tips helper" }),
    ).not.toBeInTheDocument();
  });

  it("reads the offer and every tip out to screen readers", () => {
    const spoken: string[] = [];
    const unsubscribe = subscribeAnnouncements((a) => {
      spoken.push(a.message);
    });
    render(<TipsHost />);
    act(() => {
      offerTour();
    });
    fireEvent.click(button("Show me"));
    fireEvent.click(button("Next"));
    unsubscribe();
    expect(spoken).toHaveLength(3);
    expect(spoken[0]).toContain("Hi, I'm Aaron!");
    expect(spoken[1]).toContain("Your servers");
    expect(spoken[2]).not.toContain("Your servers");
  });

  it("offers the tour and walks through every tip to the end", () => {
    render(<TipsHost />);
    act(() => {
      offerTour();
    });
    expect(bubble()).toHaveTextContent("Hi, I'm Aaron!");
    expect(screen.getByTestId("aaron")).toBeInTheDocument();
    expect(button("Show me")).toHaveFocus();

    fireEvent.click(button("Show me"));
    expect(bubble()).toHaveTextContent(`Tip 1 of ${String(TIPS.length)}`);
    expect(bubble()).toHaveTextContent("Your servers");
    expect(screen.queryByRole("button", { name: "Back" })).not.toBeInTheDocument();
    expect(button("Next")).toHaveFocus();

    for (let tip = 1; tip < TIPS.length; tip += 1) fireEvent.click(button("Next"));
    expect(bubble()).toHaveTextContent(`Tip ${String(TIPS.length)} of ${String(TIPS.length)}`);
    expect(bubble()).toHaveTextContent("That's it!");
    fireEvent.click(button("Done"));
    expect(
      screen.queryByRole("region", { name: "Aaron, the tips helper" }),
    ).not.toBeInTheDocument();
  });

  it("goes back a tip and names the search shortcut", () => {
    render(<TipsHost />);
    act(() => {
      startTour();
    });
    for (let tip = 0; tip < TIPS.indexOf("search"); tip += 1) fireEvent.click(button("Next"));
    expect(bubble()).toHaveTextContent("Press Ctrl K to find people and channels");
    fireEvent.click(button("Back"));
    expect(bubble()).toHaveTextContent("Your user panel");
  });

  it("floats next to the tip's element and points at it", () => {
    render(
      <>
        <Target id="servers" box={boxOf(0, 0, 72, 600)} />
        <Target id="apps" box={boxOf(980, 500, 40, 40)} />
        <TipsHost />
      </>,
    );
    act(() => {
      startTour();
    });
    expect(aaron()).toHaveAttribute("data-pose", "point-left");
    expect(bubble()).toHaveClass("tips--mascot-left");
    expect(bubble()).toHaveStyle({ left: `${String(72 + GAP)}px` });

    for (let tip = 0; tip < TIPS.indexOf("apps"); tip += 1) fireEvent.click(button("Next"));
    expect(bubble()).toHaveTextContent("Apps");
    expect(aaron()).toHaveAttribute("data-pose", "point-right");
    expect(bubble()).not.toHaveClass("tips--mascot-left");
  });

  it("dims the window during the tour and spotlights the tip's element", () => {
    render(
      <>
        <Target id="servers" box={boxOf(0, 0, 72, 600)} />
        <TipsHost />
      </>,
    );
    act(() => {
      offerTour();
    });
    expect(screen.queryByTestId("tips-dim")).not.toBeInTheDocument();

    fireEvent.click(button("Show me"));
    const dim = screen.getByTestId("tips-dim");
    expect(dim).toHaveClass("tips-dim--spot");
    expect(dim).toHaveAttribute("aria-hidden", "true");
    expect(dim).not.toHaveAttribute("tabindex");
    // Padded around the target, but kept inside the window.
    expect(dim).toHaveStyle({ left: "2px", top: "2px", width: "74px", height: "602px" });

    // Without an element the whole window is dimmed.
    fireEvent.click(button("Next"));
    expect(screen.getByTestId("tips-dim")).not.toHaveClass("tips-dim--spot");

    fireEvent.click(button("Skip tour"));
    expect(screen.queryByTestId("tips-dim")).not.toBeInTheDocument();
  });

  it("waits idle in the corner when the element is missing and on the last tip", () => {
    render(<TipsHost />);
    act(() => {
      offerTour();
    });
    expect(aaron()).toHaveAttribute("data-pose", "idle");
    fireEvent.click(button("Show me"));
    expect(bubble()).toHaveTextContent("Your servers");
    expect(aaron()).toHaveAttribute("data-pose", "idle");
    for (let tip = 1; tip < TIPS.length; tip += 1) fireEvent.click(button("Next"));
    expect(aaron()).toHaveAttribute("data-pose", "idle");
  });

  it("talks for a moment whenever a tip appears", () => {
    vi.useFakeTimers();
    try {
      render(<TipsHost />);
      act(() => {
        startTour();
      });
      expect(aaron()).toHaveClass("aaron--talking");
      act(() => {
        vi.advanceTimersByTime(2000);
      });
      expect(aaron()).not.toHaveClass("aaron--talking");
      fireEvent.click(button("Next"));
      expect(aaron()).toHaveClass("aaron--talking");
    } finally {
      vi.useRealTimers();
    }
  });

  it("skips the rest of the tour from any tip", () => {
    render(<TipsHost />);
    act(() => {
      startTour();
    });
    fireEvent.click(button("Next"));
    fireEvent.click(button("Skip tour"));
    expect(
      screen.queryByRole("region", { name: "Aaron, the tips helper" }),
    ).not.toBeInTheDocument();
  });

  it("closes a tip with Escape from any of its buttons", () => {
    render(<TipsHost />);
    act(() => {
      startTour();
    });
    fireEvent.click(button("Next"));
    expect(button("Next")).toHaveFocus();
    fireEvent.keyDown(button("Back"), { key: "Escape" });
    expect(
      screen.queryByRole("region", { name: "Aaron, the tips helper" }),
    ).not.toBeInTheDocument();
  });

  it.each([
    ["Not now", () => fireEvent.click(button("Not now"))],
    ["the close button", () => fireEvent.click(button("Close tips"))],
    ["Escape", () => fireEvent.keyDown(button("Show me"), { key: "Escape" })],
  ])("closes with %s", (_how, close) => {
    render(<TipsHost />);
    act(() => {
      offerTour();
    });
    close();
    expect(
      screen.queryByRole("region", { name: "Aaron, the tips helper" }),
    ).not.toBeInTheDocument();
  });

  it("has no axe violations", async () => {
    render(<TipsHost />);
    act(() => {
      startTour();
    });
    expect(await axeViolations(bubble())).toEqual([]);
  });

  it("offers the tour when the first-start wizard finishes", async () => {
    setApi(createMockApi({ seed: "fresh" }));
    function FinishWizard() {
      const { finishWizard } = useView();
      return (
        <button type="button" onClick={finishWizard}>
          Finish
        </button>
      );
    }
    render(
      <PreferencesProvider>
        <ViewProvider>
          <FinishWizard />
          <TipsHost />
        </ViewProvider>
      </PreferencesProvider>,
    );
    expect(
      screen.queryByRole("region", { name: "Aaron, the tips helper" }),
    ).not.toBeInTheDocument();
    fireEvent.click(await screen.findByRole("button", { name: "Finish" }));
    expect(bubble()).toHaveTextContent("Hi, I'm Aaron!");
  });
});

describe("Settings → About → Tips", () => {
  it("closes the settings and starts the tour", () => {
    const view = fakeView({ screen: "main", overlay: "settings" });
    render(
      <ViewContext value={view}>
        <TipsCard />
        <TipsHost />
      </ViewContext>,
    );
    fireEvent.click(button("Show tips"));
    expect(view.closeOverlay).toHaveBeenCalledOnce();
    expect(bubble()).toHaveTextContent(`Tip 1 of ${String(TIPS.length)}`);
  });
});
