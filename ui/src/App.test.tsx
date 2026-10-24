import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { axeViolations } from "./test/axe";

describe("App", () => {
  it("renders the main landmark without accessibility violations", async () => {
    const { container } = render(<App />);
    expect(screen.getByRole("main", { name: "Mumcord" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });
});
