import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import "../../i18n";
import { MumcordBadge } from "./MumcordBadge";
import { hasMumcordFeature, isMumcord } from "./mumcord";

describe("MumcordBadge", () => {
  it("renders a labelled badge for Mumcord users", () => {
    render(<MumcordBadge user={{ isMumcord: true }} />);
    expect(screen.getByRole("img", { name: "Uses Mumcord" })).toHaveAttribute(
      "title",
      "Uses Mumcord",
    );
  });

  it("renders nothing for other clients or unknown people", () => {
    const { container, rerender } = render(<MumcordBadge user={{ isMumcord: false }} />);
    expect(container).toBeEmptyDOMElement();
    rerender(<MumcordBadge user={undefined} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("gates features on the announced list", () => {
    const user = { isMumcord: true, mumcordFeatures: ["now-gaming"] };
    expect(isMumcord(user)).toBe(true);
    expect(hasMumcordFeature(user, "now-gaming")).toBe(true);
    expect(hasMumcordFeature(user, "history-sync")).toBe(false);
    expect(
      hasMumcordFeature({ isMumcord: false, mumcordFeatures: ["now-gaming"] }, "now-gaming"),
    ).toBe(false);
  });
});
