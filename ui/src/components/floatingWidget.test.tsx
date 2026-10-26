import { readFileSync } from "node:fs";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../i18n";
import { axeViolations } from "../test/axe";
import { FloatingWidget, type FloatingWidgetProps } from "./FloatingWidget";
import {
  boundsOf,
  clampRect,
  INPUT_RESERVE,
  KEY_STEP,
  MIN_SIZE,
  moveRect,
  resizeRect,
  type Rect,
  type SizeRules,
} from "./floatingWidgetGeometry";
import { getWidgetLayout, resetFloatingWidgets, Z_BASE } from "./floatingWidgetStore";

const bounds = { width: 1200, height: 800 };
const free: SizeRules = { min: MIN_SIZE, chrome: 40 };
const video: SizeRules = { min: MIN_SIZE, aspect: 16 / 9, chrome: 40 };
const start: Rect = { x: 100, y: 100, width: 480, height: 310 };

describe("geometry", () => {
  it("keeps the chat input strip free", () => {
    expect(boundsOf({ width: 1200, height: 900 })).toEqual({
      width: 1200,
      height: 900 - INPUT_RESERVE,
    });
  });

  it("keeps a moved widget inside the bounds", () => {
    expect(moveRect(start, -500, -500, bounds)).toMatchObject({ x: 0, y: 0 });
    expect(moveRect(start, 5000, 5000, bounds)).toMatchObject({
      x: bounds.width - start.width,
      y: bounds.height - start.height,
    });
  });

  it("shrinks and moves a widget back inside when the window gets smaller", () => {
    const fitted = clampRect(
      { x: 900, y: 600, width: 600, height: 400 },
      { width: 700, height: 500 },
      free,
    );
    expect(fitted).toEqual({ x: 100, y: 100, width: 600, height: 400 });
    const tiny = clampRect(start, { width: 300, height: 150 }, free);
    expect(tiny.width).toBeLessThanOrEqual(300);
    expect(tiny.height).toBeLessThanOrEqual(150);
  });

  it("never resizes below the minimum", () => {
    const small = resizeRect(start, "se", -1000, -1000, bounds, free);
    expect(small).toMatchObject({ x: 100, y: 100, ...MIN_SIZE });
  });

  it("keeps the opposite edge when resizing from the top left", () => {
    const grown = resizeRect(start, "nw", -50, -50, bounds, free);
    expect(grown).toEqual({ x: 50, y: 50, width: 530, height: 360 });
  });

  it("keeps the aspect ratio of the content below the title bar", () => {
    const grown = resizeRect(start, "se", 160, 10, bounds, video);
    expect(grown.width).toBe(640);
    expect(grown.height).toBe(640 / (16 / 9) + 40);
    const taller = resizeRect(start, "s", 0, 90, bounds, video);
    expect((taller.height - 40) * (16 / 9)).toBeCloseTo(taller.width);
    const small = resizeRect(start, "w", 1000, 0, bounds, video);
    expect(small.height).toBeGreaterThanOrEqual(MIN_SIZE.height);
    expect(small.width).toBeGreaterThanOrEqual(MIN_SIZE.width);
    expect((small.height - 40) * (16 / 9)).toBeCloseTo(small.width);
  });

  it("applies the minimum and the ratio to the video, not to the whole widget", () => {
    const party: SizeRules = { ...video, extra: 56 };
    const minHeight = MIN_SIZE.height + 40 + 56;
    for (const edge of ["n", "s", "e", "w", "ne", "nw", "se", "sw"] as const) {
      const r = resizeRect(
        { x: 300, y: 300, width: 480, height: 480 / (16 / 9) + 96 },
        edge,
        edge.includes("w") ? 2000 : -2000,
        edge.includes("n") ? 2000 : -2000,
        bounds,
        party,
      );
      expect(r.width).toBeCloseTo(MIN_SIZE.width);
      expect(r.height).toBeCloseTo(minHeight);
    }
    const grown = resizeRect(start, "se", 160, 10, bounds, party);
    expect(grown.height).toBeCloseTo(grown.width / (16 / 9) + 96);
    const fromTop = resizeRect(start, "n", 0, -90, bounds, party);
    expect((fromTop.height - 96) * (16 / 9)).toBeCloseTo(fromTop.width);
  });

  it("does not grow past the window", () => {
    const grown = resizeRect(start, "se", 5000, 5000, bounds, video);
    expect(grown.x + grown.width).toBeLessThanOrEqual(bounds.width);
    expect(grown.y + grown.height).toBeLessThanOrEqual(bounds.height);
  });
});

describe("FloatingWidget", () => {
  beforeEach(async () => {
    await applyLanguage("en");
    // jsdom has no pointer capture.
    Element.prototype.setPointerCapture = capture;
    Element.prototype.hasPointerCapture = () => false;
    Element.prototype.releasePointerCapture = vi.fn();
  });
  afterEach(() => {
    resetFloatingWidgets();
  });

  const capture = vi.fn();

  function Widget(props: Partial<FloatingWidgetProps>) {
    return (
      <FloatingWidget
        kind="test"
        label="Test widget"
        title="Title"
        initialRect={() => ({ ...start })}
        {...props}
      >
        <p>content</p>
      </FloatingWidget>
    );
  }

  const handle = (name = "Test widget") =>
    screen.getByLabelText(`${name}: arrow keys move, Shift+arrow keys resize`);
  const region = (name = "Test widget") => screen.getByRole("region", { name });

  it("has no accessibility violations", async () => {
    const { container } = render(<Widget close={{ label: "Close", onClose: () => undefined }} />);
    expect(await axeViolations(container)).toEqual([]);
  });

  it("moves with the arrow keys and resizes with Shift+arrow keys", () => {
    render(<Widget />);
    const bar = handle();
    expect(bar).toHaveAttribute("tabindex", "0");
    fireEvent.keyDown(bar, { key: "ArrowRight" });
    fireEvent.keyDown(bar, { key: "ArrowDown" });
    expect(getWidgetLayout("test")?.rect).toMatchObject({ x: 100 + KEY_STEP, y: 100 + KEY_STEP });
    fireEvent.keyDown(bar, { key: "ArrowRight", shiftKey: true });
    expect(getWidgetLayout("test")?.rect.width).toBe(480 + KEY_STEP);
    fireEvent.keyDown(bar, { key: "ArrowUp", shiftKey: true });
    expect(getWidgetLayout("test")?.rect.height).toBe(310 - KEY_STEP);
    expect(region()).toHaveStyle({
      left: `${String(100 + KEY_STEP)}px`,
      width: `${String(480 + KEY_STEP)}px`,
    });
  });

  it("drags by the title bar and resizes from a corner", () => {
    const { container } = render(<Widget />);
    fireEvent.pointerDown(handle(), { button: 0, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(window, { clientX: 60, clientY: 30 });
    fireEvent.pointerUp(window);
    expect(getWidgetLayout("test")?.rect).toMatchObject({ x: 150, y: 120 });
    const corner = container.querySelector('[data-edge="se"]');
    if (corner === null) throw new Error("no corner");
    fireEvent.pointerDown(corner, { button: 0, clientX: 0, clientY: 0 });
    fireEvent.pointerMove(window, { clientX: 40, clientY: 20 });
    fireEvent.pointerUp(window);
    fireEvent.pointerMove(window, { clientX: 400, clientY: 400 });
    expect(getWidgetLayout("test")?.rect).toMatchObject({
      x: 150,
      y: 120,
      width: 520,
      height: 330,
    });
  });

  it("ends a gesture on pointerup, pointercancel, blur and lost capture", () => {
    render(<Widget />);
    const ends: [string, () => void][] = [
      ["pointerup", () => fireEvent.pointerUp(window)],
      ["pointercancel", () => fireEvent.pointerCancel(window)],
      ["blur", () => fireEvent.blur(window)],
      ["lostpointercapture", () => fireEvent.lostPointerCapture(handle())],
    ];
    for (const [, end] of ends) {
      fireEvent.pointerDown(handle(), { button: 0, clientX: 0, clientY: 0 });
      expect(screen.getByRole("region", { name: "Test widget" })).toHaveAttribute(
        "data-gesture",
        "move",
      );
      end();
      expect(screen.getByRole("region", { name: "Test widget" })).not.toHaveAttribute(
        "data-gesture",
      );
    }
  });

  it("applies a fast 300px jump in two steps when the pointer is released", () => {
    render(<Widget />);
    const before = 100;
    fireEvent.pointerDown(handle(), { button: 0, clientX: 0, clientY: 0 });
    fireEvent.pointerMove(window, { clientX: 150, clientY: 0 });
    fireEvent.pointerMove(window, { clientX: 300, clientY: 0 });
    fireEvent.pointerUp(window);
    expect(getWidgetLayout("test")?.rect.x).toBe(before + 300);
  });

  it("drags from the title text and the free bar space, not from the title extra", () => {
    render(<Widget titleExtra={<span>avatars</span>} />);
    const drag = (target: Element) => {
      fireEvent.pointerDown(target, { button: 0, clientX: 10, clientY: 10, pointerId: 1 });
      fireEvent.pointerMove(window, { clientX: 20, clientY: 10 });
      fireEvent.pointerUp(window);
    };
    drag(screen.getByText("Title"));
    expect(capture).toHaveBeenCalledWith(1);
    drag(handle());
    expect(getWidgetLayout("test")?.rect.x).toBe(120);
    drag(screen.getByText("avatars"));
    expect(getWidgetLayout("test")?.rect.x).toBe(120);
  });

  it("leaves the free bar space to the drag and keeps iframes out of a gesture", () => {
    // Regression F35: a growing no-drag extra covered the whole free bar space.
    const css = readFileSync("ui/src/components/floatingWidget.css", "utf8");
    const rule = (selector: string) =>
      css.match(new RegExp(`${selector.replace(/[.[\]]/g, "\\$&")} \\{([^}]*)\\}`))?.[1] ?? "";
    expect(rule(".floating-widget__title-extra")).toMatch(/flex: 0 1 auto;/);
    expect(rule(".floating-widget__title-extra")).toMatch(/margin-inline-end: auto;/);
    expect(rule(".floating-widget[data-gesture] .floating-widget__body")).toMatch(
      /pointer-events: none;/,
    );
  });

  it("collapses to a slim bar and expands again, keeping the content mounted", () => {
    render(<Widget collapsedTitle="Slim" collapsedActions={<button type="button">Play</button>} />);
    const content = screen.getByText("content");
    fireEvent.click(screen.getByRole("button", { name: "Collapse" }));
    expect(content).not.toBeVisible();
    expect(screen.getByText("Slim")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Play" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Expand" }));
    expect(screen.getByText("content")).toBe(content);
    expect(content).toBeVisible();
    expect(screen.queryByRole("button", { name: "Play" })).toBeNull();
  });

  it("closes through its close action", () => {
    let closed = false;
    render(
      <Widget
        close={{
          label: "Stop watching",
          onClose: () => {
            closed = true;
          },
        }}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Stop watching" }));
    expect(closed).toBe(true);
  });

  it("puts the last clicked widget on top, below menus", () => {
    render(
      <>
        <Widget kind="a" label="A" />
        <Widget kind="b" label="B" />
      </>,
    );
    const z = (name: string) => Number(region(name).style.zIndex);
    expect(z("B")).toBeGreaterThan(z("A"));
    fireEvent.pointerDown(region("A"));
    expect(z("A")).toBeGreaterThan(z("B"));
    expect(z("A")).toBeGreaterThanOrEqual(Z_BASE);
    expect(z("A")).toBeLessThan(45);
  });

  it("remembers position and size per widget type for the session", () => {
    const view = render(<Widget />);
    fireEvent.keyDown(handle(), { key: "ArrowLeft" });
    view.unmount();
    render(<Widget />);
    expect(region()).toHaveStyle({ left: `${String(100 - KEY_STEP)}px` });
  });

  it("stays inside the window when it gets smaller", () => {
    render(<Widget />);
    const { innerWidth, innerHeight } = window;
    act(() => {
      window.innerWidth = 500;
      window.innerHeight = 400;
      window.dispatchEvent(new Event("resize"));
    });
    const style = region().style;
    expect(parseFloat(style.left) + parseFloat(style.width)).toBeLessThanOrEqual(500);
    expect(parseFloat(style.top) + parseFloat(style.height)).toBeLessThanOrEqual(
      400 - INPUT_RESERVE,
    );
    act(() => {
      window.innerWidth = innerWidth;
      window.innerHeight = innerHeight;
      window.dispatchEvent(new Event("resize"));
    });
  });
});
