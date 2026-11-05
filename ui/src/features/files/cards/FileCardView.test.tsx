import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type FileCard, type MockApi } from "../../../api";
import { subscribeAnnouncements } from "../../../a11y/announcer";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { stubDialog } from "../../channels/testUtils";
import { FileCardView } from "./FileCardView";

const HASH = "3f9a1c7e5b2d8046a9e3c1f7d5b8a24e60c9f1d3b7a5e8c2406d9b1f3a7ec21e";

function card(overrides: Partial<FileCard> = {}): FileCard {
  return {
    shareId: "s1",
    name: "game-night-slides.pdf",
    size: 19_293_798,
    mime: "application/pdf",
    sha256: HASH,
    expires: Math.floor(Date.now() / 1000) + 3600,
    senderName: "Alice",
    own: false,
    risky: false,
    state: { kind: "available" },
    downloadBlock: null,
    share: null,
    ...overrides,
  };
}

let api: MockApi;

beforeAll(async () => {
  stubDialog();
  await applyLanguage("en");
});

beforeEach(() => {
  // The cards say "ends today at" only when the expiry is on the same day, so a test run just
  // before midnight would see "tomorrow". Pin the date to midday; timers keep running for real.
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 5, 15, 12, 0, 0));
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("received file card", () => {
  it("shows the offer with name, size, type, short checksum and Download (mockup 2)", async () => {
    const { container } = render(<FileCardView card={card()} />);
    const group = screen.getByRole("group", {
      name: "game-night-slides.pdf, 18.4 MB, from Alice, available",
    });
    expect(group).toHaveTextContent("18.4 MB · PDF document");
    expect(group).toHaveTextContent("3f9a…c21e");
    expect(group).toHaveTextContent(`SHA-256 checksum ${HASH}`);
    expect(group).toHaveTextContent(/Available while Alice is online · ends today at/);
    expect(screen.getByRole("button", { name: "Download" })).toBeEnabled();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("asks before every download and starts it only on confirm (FR-7.22)", async () => {
    const download = vi.spyOn(api, "filesDownload");
    render(<FileCardView card={card()} />);
    fireEvent.click(screen.getByRole("button", { name: "Download" }));
    const dialog = screen.getByRole("dialog", { name: "Download this file?" });
    expect(dialog).toHaveTextContent(HASH);
    expect(dialog).toHaveTextContent("Alice");
    await waitFor(() => {
      expect(dialog).toHaveTextContent("~/Downloads");
    });
    const confirm = within(dialog).getByRole("button", { name: "Download" });
    expect(confirm).toHaveFocus();
    expect(download).not.toHaveBeenCalled();
    fireEvent.click(confirm);
    expect(download).toHaveBeenCalledWith(expect.any(Number), "s1");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("cancels the confirmation without downloading", () => {
    const download = vi.spyOn(api, "filesDownload");
    render(<FileCardView card={card()} />);
    fireEvent.click(screen.getByRole("button", { name: "Download" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(download).not.toHaveBeenCalled();
  });

  it("warns about a program with the safe answer focused (mockup 6)", () => {
    const download = vi.spyOn(api, "filesDownload");
    render(<FileCardView card={card({ name: "setup.exe", risky: true, size: 54_630_195 })} />);
    expect(screen.getByRole("group")).toHaveTextContent("52.1 MB · Windows program");
    fireEvent.click(screen.getByRole("button", { name: "Download" }));
    const dialog = screen.getByRole("alertdialog", { name: "This file could harm your computer" });
    expect(dialog).toHaveTextContent("setup.exe · 52.1 MB · from Alice");
    expect(screen.getByRole("button", { name: "Don't download" })).toHaveFocus();
    fireEvent.click(screen.getByRole("button", { name: "Download anyway" }));
    expect(download).toHaveBeenCalledWith(expect.any(Number), "s1");
  });

  it("disables Download with the reason as description (mockup 13, FR-7.21)", () => {
    const download = vi.spyOn(api, "filesDownload");
    render(
      <FileCardView
        card={card({ downloadBlock: { kind: "unavailable", reason: { kind: "noRelay" } } })}
      />,
    );
    const button = screen.getByRole("button", { name: "Download" });
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).toHaveAccessibleDescription(
      "This server has no relay, so file sharing isn't available.",
    );
    fireEvent.click(button);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(download).not.toHaveBeenCalled();
  });

  it("links to the setting when the file is over the limit", () => {
    render(<FileCardView card={card({ downloadBlock: { kind: "tooLarge", limit: "100M" } })} />);
    expect(screen.getByRole("button", { name: "Download" })).toHaveAccessibleDescription(
      "Larger than your limit of 100 MB",
    );
    expect(screen.getByRole("button", { name: "Change the limit" })).toBeInTheDocument();
  });

  it("shows progress with a progressbar and Cancel (mockup 3, FR-7.28)", () => {
    const cancel = vi.spyOn(api, "filesCancel");
    render(
      <FileCardView
        card={card({
          state: { kind: "downloading", received: 11_953_766, bytesPerSec: 5_033_165 },
        })}
      />,
    );
    const bar = screen.getByRole("progressbar", { name: "Downloading game-night-slides.pdf" });
    expect(bar).toHaveAttribute("aria-valuenow", "61");
    expect(screen.getByText("61 % · 11.4 of 18.4 MB · 4.8 MB/s · about 2 s left")).toBeVisible();
    expect(screen.getByText("Peer-to-peer · end-to-end encrypted")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(cancel).toHaveBeenCalledWith(expect.any(Number), "s1");
  });

  it("announces state changes politely, not the first render", () => {
    const spoken: string[] = [];
    const stop = subscribeAnnouncements(({ message, politeness }) => {
      if (politeness === "polite") spoken.push(message);
    });
    const { rerender } = render(<FileCardView card={card({ state: { kind: "checking" } })} />);
    rerender(<FileCardView card={card({ state: { kind: "done", path: "/d/x.pdf" } })} />);
    stop();
    expect(spoken).toEqual(["game-night-slides.pdf: downloaded"]);
  });

  it("shows the queue position", () => {
    render(<FileCardView card={card({ state: { kind: "queued", position: 2 } })} />);
    expect(screen.getByText("Queued (position 2)")).toBeVisible();
  });

  it("shows the full checksum and Open / Open folder when done (mockup 4, FR-7.29)", () => {
    const open = vi.spyOn(api, "filesOpen");
    const folder = vi.spyOn(api, "filesOpenFolder");
    render(
      <FileCardView
        card={card({ state: { kind: "done", path: "/home/bob/Downloads/game-night-slides.pdf" } })}
      />,
    );
    const group = screen.getByRole("group");
    expect(group).toHaveTextContent("✓ Downloaded✓ Matches the offer");
    expect(group).toHaveTextContent("Saved to /home/bob/Downloads");
    expect(screen.getAllByText(HASH)).not.toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "Open folder" }));
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(folder).toHaveBeenCalledWith(expect.any(Number), "s1");
    expect(open).toHaveBeenCalledWith(expect.any(Number), "s1");
  });

  it("asks once more before opening a program", () => {
    const open = vi.spyOn(api, "filesOpen");
    render(
      <FileCardView
        card={card({
          name: "setup.exe",
          risky: true,
          state: { kind: "done", path: "/home/bob/Downloads/setup.exe" },
        })}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(screen.getByRole("alertdialog")).toHaveTextContent(
      "setup.exe is a program or script. Only open it if you trust Alice.",
    );
    expect(open).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Open anyway" }));
    expect(open).toHaveBeenCalledWith(expect.any(Number), "s1");
  });

  it("tells when the downloaded file was moved", async () => {
    api.mock.failNext("filesOpen", "notFound");
    render(
      <FileCardView card={card({ state: { kind: "done", path: "/home/bob/Downloads/a.pdf" } })} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The file is no longer in /home/bob/Downloads",
    );
  });

  it("shows a mismatch as an alert, and Dismiss offers the download again (mockup 12)", () => {
    render(<FileCardView card={card({ state: { kind: "mismatch" } })} />);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Hash mismatch – file deletedThe file changed in transit or doesn't match the offer – deleted. Nothing was saved or opened. Ask Alice to share it again.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("button", { name: "Download" })).toBeVisible();
  });

  it("shows a failed download with its reason and Retry (FR-7.20, FR-7.34)", async () => {
    const download = vi.spyOn(api, "filesDownload");
    render(
      <FileCardView card={card({ state: { kind: "failed", reason: { kind: "noAnswer" } } })} />,
    );
    expect(screen.getByText("Download failed: Alice did not answer")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    // Download unlocks once the confirmation shows the save folder.
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Download" })).not.toHaveAttribute(
        "aria-disabled",
        "true",
      );
    });
    fireEvent.click(screen.getByRole("button", { name: "Download" }));
    expect(download).toHaveBeenCalled();
  });

  it("says the sender went offline and disables Download (mockup 7)", () => {
    render(
      <FileCardView card={card({ state: { kind: "unavailable", reason: "senderOffline" } })} />,
    );
    expect(screen.getByText("Alice went offline. The file is no longer available.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Download" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("says an ended share has no Download (mockup 7)", () => {
    render(<FileCardView card={card({ state: { kind: "unavailable", reason: "expired" } })} />);
    expect(screen.getByText("This share ended. Shares end after 24 hours.")).toBeVisible();
    expect(screen.getByText("Ask Alice to share it again.")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Download" })).toBeNull();
  });
});

describe("own file card", () => {
  const own = (share: NonNullable<FileCard["share"]>) =>
    card({ own: true, downloadBlock: { kind: "own" }, share });

  it("shows who downloaded it, the running transfer and Stop sharing (mockup 5)", async () => {
    const stop = vi.spyOn(api, "filesStop");
    const { container } = render(
      <FileCardView
        card={own({
          ended: false,
          endReason: null,
          downloadedBy: ["Bob", "Carol"],
          sendingTo: { recipient: "Dave", percent: 40 },
          waiting: 0,
        })}
      />,
    );
    const group = screen.getByRole("group", {
      name: "game-night-slides.pdf, 18.4 MB, shared by you, available",
    });
    expect(group).toHaveTextContent("18.4 MB · PDF document · shared by you");
    expect(group).toHaveTextContent("Downloaded by Bob and Carol");
    expect(group).toHaveTextContent("1 download in progress · Dave 40 %");
    expect(group).toHaveTextContent(/Sharing while you are online · ends today at/);
    expect(screen.queryByRole("button", { name: "Download" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Stop sharing" }));
    expect(stop).toHaveBeenCalledWith("s1");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows a failed Stop sharing as an alert", async () => {
    vi.spyOn(api, "filesStop").mockRejectedValue(new Error("engine gone"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(
      <FileCardView
        card={own({ ended: false, endReason: null, downloadedBy: [], sendingTo: null, waiting: 0 })}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Stop sharing" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Couldn't stop sharing: engine gone. The file is still shared.",
    );
  });

  it("shows waiting requests", () => {
    render(
      <FileCardView
        card={own({
          ended: false,
          endReason: null,
          downloadedBy: [],
          sendingTo: { recipient: "Dave", percent: 5 },
          waiting: 2,
        })}
      />,
    );
    expect(screen.getByText("1 download in progress · Dave 5 % · 2 waiting")).toBeVisible();
  });

  it("stays in the chat after the end with the reason", () => {
    render(
      <FileCardView
        card={own({
          ended: true,
          endReason: "fileChanged",
          downloadedBy: [],
          sendingTo: null,
          waiting: 0,
        })}
      />,
    );
    expect(
      screen.getByText("Sharing ended · The file changed on disk, so sharing stopped"),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "Stop sharing" })).toBeNull();
  });

  it("shows a plain 'Sharing ended' after quitting", () => {
    render(
      <FileCardView
        card={own({
          ended: true,
          endReason: "quit",
          downloadedBy: [],
          sendingTo: null,
          waiting: 0,
        })}
      />,
    );
    expect(screen.getByText("Sharing ended")).toBeVisible();
  });
});
