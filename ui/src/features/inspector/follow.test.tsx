import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { countUnseen, isAtBottom, useFollowScroll } from "./follow";
import type { Packet } from "./packets";

function packet(id: number): Packet {
  return {
    id,
    serverId: 1,
    atMs: 0,
    direction: "sent",
    kind: "Ping",
    pluginData: false,
    size: 1,
    sender: null,
    receivers: [],
    delayMs: null,
    tokensLeft: null,
    decoded: null,
    rawHex: "",
  };
}

const list = (n: number): Packet[] => Array.from({ length: n }, (_, i) => packet(i));

describe("isAtBottom", () => {
  it("is true at the end and within a few pixels of it", () => {
    expect(isAtBottom({ scrollTop: 600, clientHeight: 400, scrollHeight: 1000 })).toBe(true);
    expect(isAtBottom({ scrollTop: 590, clientHeight: 400, scrollHeight: 1000 })).toBe(true);
  });

  it("is false once the user has scrolled up", () => {
    expect(isAtBottom({ scrollTop: 300, clientHeight: 400, scrollHeight: 1000 })).toBe(false);
  });

  it("is true when nothing overflows", () => {
    expect(isAtBottom({ scrollTop: 0, clientHeight: 400, scrollHeight: 300 })).toBe(true);
  });
});

describe("countUnseen", () => {
  it("counts the packets newer than the last one seen", () => {
    expect(countUnseen(list(5), 2)).toBe(2);
    expect(countUnseen(list(5), 4)).toBe(0);
  });

  it("counts everything when nothing was seen", () => {
    expect(countUnseen(list(3), null)).toBe(3);
  });
});

function Harness({ packets }: { packets: Packet[] }) {
  const { attachScroller, onScroll, follow, unseen, setFollow, pause, jump } =
    useFollowScroll(packets);
  return (
    <>
      <div data-testid="scroller" ref={attachScroller} onScroll={onScroll} />
      <span data-testid="state">{`${String(follow)} ${String(unseen)}`}</span>
      <button
        type="button"
        onClick={() => {
          setFollow(!follow);
        }}
      >
        toggle
      </button>
      <button type="button" onClick={pause}>
        pause
      </button>
      <button type="button" onClick={jump}>
        jump
      </button>
    </>
  );
}

function geometry(el: HTMLElement, scrollTop: number): void {
  Object.defineProperty(el, "clientHeight", { configurable: true, value: 400 });
  Object.defineProperty(el, "scrollHeight", { configurable: true, value: 1000 });
  el.scrollTop = scrollTop;
}

describe("useFollowScroll", () => {
  const state = () => screen.getByTestId("state").textContent;

  it("follows new packets to the bottom by default", () => {
    const { rerender } = render(<Harness packets={list(2)} />);
    const scroller = screen.getByTestId("scroller");
    geometry(scroller, 0);
    rerender(<Harness packets={list(3)} />);
    expect(scroller.scrollTop).toBe(1000);
    expect(state()).toBe("true 0");
  });

  it("pauses when the user scrolls up and counts the packets that arrive meanwhile", () => {
    const { rerender } = render(<Harness packets={list(2)} />);
    const scroller = screen.getByTestId("scroller");
    geometry(scroller, 100);
    fireEvent.scroll(scroller);
    expect(state()).toBe("false 0");
    rerender(<Harness packets={list(5)} />);
    expect(state()).toBe("false 3");
    expect(scroller.scrollTop).toBe(100);
  });

  it("resumes when the user scrolls back to the bottom", () => {
    const { rerender } = render(<Harness packets={list(2)} />);
    const scroller = screen.getByTestId("scroller");
    geometry(scroller, 100);
    fireEvent.scroll(scroller);
    rerender(<Harness packets={list(4)} />);
    geometry(scroller, 600);
    fireEvent.scroll(scroller);
    expect(state()).toBe("true 0");
  });

  it("jumps to the latest packet and follows again", () => {
    const { rerender } = render(<Harness packets={list(2)} />);
    const scroller = screen.getByTestId("scroller");
    geometry(scroller, 100);
    fireEvent.scroll(scroller);
    rerender(<Harness packets={list(4)} />);
    fireEvent.click(screen.getByText("jump"));
    expect(state()).toBe("true 0");
    expect(scroller.scrollTop).toBe(1000);
  });

  it("can be switched off and on without scrolling", () => {
    render(<Harness packets={list(2)} />);
    fireEvent.click(screen.getByText("toggle"));
    expect(state()).toBe("false 0");
    fireEvent.click(screen.getByText("toggle"));
    expect(state()).toBe("true 0");
  });

  it("stops following when asked to pause, for example when a row is expanded", () => {
    render(<Harness packets={list(2)} />);
    act(() => {
      fireEvent.click(screen.getByText("pause"));
    });
    expect(state()).toBe("false 0");
  });
});
