import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "../../i18n";
import { axeViolations } from "../../test/axe";
import { P2pModeRadios } from "./P2pModeRadios";

describe("P2pModeRadios", () => {
  it("lists the five modes with the default recommended and the explanation", async () => {
    const { container } = render(<P2pModeRadios value="directRelay" onChange={vi.fn()} />);
    const group = screen.getByRole("group", { name: "Peer-to-peer mode" });
    const radios = within(group).getAllByRole("radio");
    expect(radios.map((radio) => radio.closest("label")?.textContent)).toEqual([
      "Direct + relay (direct first) Recommended",
      "Direct + relay (always relay, hides my IP)",
      "Direct only",
      "Relay only",
      "Off",
    ]);
    expect(within(group).getByRole("radio", { name: /direct first.*Recommended/ })).toBeChecked();
    expect(group).toHaveAccessibleDescription(/Direct shows your IP address to the other person/);
    expect(await axeViolations(container)).toEqual([]);
  });

  it("reports the chosen mode", () => {
    const onChange = vi.fn();
    render(<P2pModeRadios value="directRelay" onChange={onChange} />);
    fireEvent.click(screen.getByRole("radio", { name: "Relay only" }));
    expect(onChange).toHaveBeenCalledWith("relayOnly");
  });

  it('offers "Use global setting" for a server override', () => {
    const onChange = vi.fn();
    render(<P2pModeRadios value={null} allowGlobal onChange={onChange} />);
    expect(screen.getAllByRole("radio")).toHaveLength(6);
    expect(screen.getByRole("radio", { name: "Use global setting" })).toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: "Off" }));
    expect(onChange).toHaveBeenCalledWith("off");
    fireEvent.click(screen.getByRole("radio", { name: "Use global setting" }));
    expect(onChange).not.toHaveBeenCalledWith(undefined);
  });

  it("keeps the title for screen readers when the heading shows it", () => {
    render(<P2pModeRadios value="off" hideTitle onChange={vi.fn()} />);
    expect(screen.getByText("Peer-to-peer (screen sharing, full-size images)")).toHaveClass(
      "visually-hidden",
    );
  });
});
