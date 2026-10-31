import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import "../../i18n";
import { P2pLinkText } from "./P2pLinkText";

describe("P2pLinkText", () => {
  it("shows a direct connection with a visible IP", () => {
    const { container } = render(<P2pLinkText link={{ path: "direct", ipHidden: false }} />);
    expect(container.textContent).toBe("Direct · IP visible");
    expect(container.querySelector("[data-tooltip]")).toBeNull();
  });

  it("shows the relay transport and explains a hidden IP", () => {
    const { container } = render(<P2pLinkText link={{ path: "relayTcp", ipHidden: true }} />);
    expect(container.textContent).toMatch(/^Via relay \(TCP 443\) · IP hidden/);
    expect(screen.getByText("IP hidden", { exact: false })).toHaveAttribute(
      "data-tooltip",
      "The other person sees the relay's address, not yours.",
    );
  });

  it("names the UDP relay", () => {
    const { container } = render(<P2pLinkText link={{ path: "relayUdp", ipHidden: false }} />);
    expect(container.textContent).toBe("Via relay (UDP) · IP visible");
  });
});
