import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { PinnedInfoBox } from "./PinnedInfo";

/** jsdom has no layout: pretend the collapsed box hides content (or not). */
function fakeOverflow(overflows: boolean) {
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(overflows ? 200 : 40);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(40);
}

beforeAll(async () => {
  await applyLanguage("en");
});

afterEach(() => {
  setApi(undefined);
  vi.restoreAllMocks();
});

describe("PinnedInfoBox", () => {
  it("renders nothing without text", () => {
    const { container } = render(<PinnedInfoBox blocks={[]} />);
    expect(container).toBeEmptyDOMElement();
    const { container: blank } = render(
      <PinnedInfoBox blocks={[{ html: "<script>x()</script>" }]} />,
    );
    expect(blank).toBeEmptyDOMElement();
  });

  it("starts collapsed for long text and toggles with an accessible button", async () => {
    fakeOverflow(true);
    const { container } = render(<PinnedInfoBox blocks={[{ html: "<p>Long rules</p>" }]} />);
    const region = screen.getByRole("region", { name: "Channel info" });
    expect(region).toHaveTextContent("Long rules");
    const toggle = screen.getByRole("button", { name: "Show more" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    const body = document.getElementById(toggle.getAttribute("aria-controls") ?? "");
    expect(body).toHaveClass("pinned-info__body--collapsed");

    fireEvent.click(toggle);
    expect(screen.getByRole("button", { name: "Show less" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(body).not.toHaveClass("pinned-info__body--collapsed");
    expect(await axeViolations(container)).toEqual([]);

    fireEvent.click(screen.getByRole("button", { name: "Show less" }));
    expect(screen.getByRole("button", { name: "Show more" })).toBeInTheDocument();
  });

  it("has no toggle when everything fits", () => {
    fakeOverflow(false);
    render(<PinnedInfoBox blocks={[{ html: "Short" }]} />);
    expect(screen.getByRole("region", { name: "Channel info" })).toHaveTextContent("Short");
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("sanitizes server HTML and opens safe links in the browser", () => {
    fakeOverflow(false);
    render(
      <PinnedInfoBox
        blocks={[
          {
            html:
              '<b>Rules</b><script>alert(1)</script><img src="https://evil.example/x.png" onerror="x()">' +
              '<a href="javascript:alert(1)">bad</a> <a href="https://example.org/guide">guide</a>',
          },
          { html: "Welcome" },
        ]}
      />,
    );
    const region = screen.getByRole("region", { name: "Channel info" });
    expect(region.innerHTML).not.toMatch(/script|onerror|evil\.example|javascript:/);
    expect(region.querySelector("b")).toHaveTextContent("Rules");
    expect(region).toHaveTextContent("Welcome");
    expect(screen.getByText("bad")).not.toHaveAttribute("href");

    const api = createMockApi();
    setApi(api);
    const openLink = vi.spyOn(api, "openLink").mockResolvedValue(null);
    fireEvent.click(screen.getByRole("link", { name: "guide" }));
    expect(openLink).toHaveBeenCalledWith("https://example.org/guide");
  });
});

describe("PinnedInfoBox close buttons", () => {
  it("offers × only on blocks that can be closed, each with its own handler", () => {
    const onHideWelcome = vi.fn();
    const onHideDescription = vi.fn();
    const { rerender } = render(<PinnedInfoBox blocks={[{ html: "<p>Rules</p>" }]} />);
    expect(screen.queryByRole("button", { name: /^Hide/ })).toBeNull();
    rerender(
      <PinnedInfoBox
        blocks={[
          {
            html: "<p>About</p>",
            hideLabel: "Hide channel description",
            onHide: onHideDescription,
          },
          { html: "<p>Hi</p>", hideLabel: "Hide welcome message", onHide: onHideWelcome },
        ]}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Hide welcome message" }));
    expect(onHideWelcome).toHaveBeenCalledOnce();
    expect(onHideDescription).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Hide channel description" }));
    expect(onHideDescription).toHaveBeenCalledOnce();
  });
});
