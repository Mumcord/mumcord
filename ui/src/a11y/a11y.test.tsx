import { act, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { announce, LiveRegions } from ".";

describe("announcer", () => {
  it("speaks polite and assertive messages in separate live regions", () => {
    render(<LiveRegions />);
    act(() => {
      announce("Anna is talking");
      announce("Connection lost", "assertive");
    });
    expect(screen.getByTestId("live-polite")).toHaveTextContent("Anna is talking");
    expect(screen.getByTestId("live-polite")).toHaveAttribute("aria-live", "polite");
    expect(screen.getByTestId("live-assertive")).toHaveTextContent("Connection lost");
  });

  it("changes the region text when the same message repeats", () => {
    render(<LiveRegions />);
    act(() => {
      announce("Ben joined");
    });
    const first = screen.getByTestId("live-polite").textContent;
    act(() => {
      announce("Ben joined");
    });
    expect(screen.getByTestId("live-polite").textContent).not.toBe(first);
    expect(screen.getByTestId("live-polite")).toHaveTextContent("Ben joined");
  });
});
