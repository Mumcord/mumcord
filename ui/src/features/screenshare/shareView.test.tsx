/** Opening a watched share and the mini bar that remains (issue #163, FR-3.125). */
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { connectedMock, resetAll } from "../channels/testUtils";
import { leaveCinema, mediaView, resetMediaView } from "../media/mediaView";
import { ShareMiniBar } from "./ShareMiniBar";
import { resetWatchingStore, startWatching } from "./watching";

const ANNA = 2;

describe("watched share", () => {
  beforeEach(async () => {
    await connectedMock({ bindStore: true });
    resetWatchingStore();
    resetMediaView();
  });

  afterEach(() => {
    resetAll();
  });

  it("opens in the main column, so nothing shows as the mini bar", () => {
    startWatching(1, ANNA);
    expect(mediaView().cinema).toBe("share");
    render(<ShareMiniBar serverId={1} />);
    expect(screen.queryByRole("group")).toBeNull();
  });

  it("remains as the mini bar after leaving, and Open goes back", () => {
    startWatching(1, ANNA);
    leaveCinema();
    render(<ShareMiniBar serverId={1} />);
    const bar = screen.getByRole("group", { name: /Screen share of/ });
    expect(bar).toHaveTextContent("Live");
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(mediaView().cinema).toBe("share");
  });

  it("stop watching closes the share and the mini bar", () => {
    startWatching(1, ANNA);
    leaveCinema();
    render(<ShareMiniBar serverId={1} />);
    fireEvent.click(screen.getByRole("button", { name: "Stop watching" }));
    expect(mediaView().cinema).toBeNull();
    expect(screen.queryByRole("group")).toBeNull();
  });
});
