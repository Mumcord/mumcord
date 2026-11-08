import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { createMockApi, setApi } from "../../api";
import { PreferencesProvider } from "../../app/PreferencesProvider";
import { fakeView } from "../../app/testView";
import { useView, ViewContext } from "../../app/view";
import { ViewProvider } from "../../app/ViewProvider";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { TipsCard } from "../settings/sections/AboutSection";
import { TipsHost } from "./TipsHost";
import { closeTour, offerTour, startTour, TIPS } from "./tour";

const dialog = () => screen.getByRole("dialog", { name: "Clippy, the tips helper" });
const button = (name: string) => screen.getByRole("button", { name });

beforeAll(async () => {
  await applyLanguage("en");
});

afterEach(() => {
  act(() => {
    closeTour();
  });
  setApi(undefined);
});

describe("Clippy's tips", () => {
  it("shows nothing until the tour is offered or started", () => {
    render(<TipsHost />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("offers the tour and walks through every tip to the end", () => {
    render(<TipsHost />);
    act(() => {
      offerTour();
    });
    expect(dialog()).toHaveTextContent("Hi, I'm Clippy!");
    expect(screen.getByTestId("clippy")).toBeInTheDocument();
    expect(button("Show me")).toHaveFocus();

    fireEvent.click(button("Show me"));
    expect(dialog()).toHaveTextContent(`Tip 1 of ${String(TIPS.length)}`);
    expect(dialog()).toHaveTextContent("Your servers");
    expect(screen.queryByRole("button", { name: "Back" })).not.toBeInTheDocument();
    expect(button("Next")).toHaveFocus();

    for (let tip = 1; tip < TIPS.length; tip += 1) fireEvent.click(button("Next"));
    expect(dialog()).toHaveTextContent(`Tip ${String(TIPS.length)} of ${String(TIPS.length)}`);
    expect(dialog()).toHaveTextContent("That's it!");
    fireEvent.click(button("Done"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("goes back a tip and names the search shortcut", () => {
    render(<TipsHost />);
    act(() => {
      startTour();
    });
    for (let tip = 0; tip < TIPS.indexOf("search"); tip += 1) fireEvent.click(button("Next"));
    expect(dialog()).toHaveTextContent("Press Ctrl K to find people and channels");
    fireEvent.click(button("Back"));
    expect(dialog()).toHaveTextContent("Your user panel");
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
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("has no axe violations", async () => {
    render(<TipsHost />);
    act(() => {
      startTour();
    });
    expect(await axeViolations(dialog())).toEqual([]);
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
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.click(await screen.findByRole("button", { name: "Finish" }));
    expect(dialog()).toHaveTextContent("Hi, I'm Clippy!");
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
    expect(dialog()).toHaveTextContent(`Tip 1 of ${String(TIPS.length)}`);
  });
});
