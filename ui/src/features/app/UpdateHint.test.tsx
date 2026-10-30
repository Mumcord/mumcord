import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../i18n";
import type { UpdateInfo } from "../../bindings/commands";
import { axeViolations } from "../../test/axe";
import { UpdateHint, type UpdateHintApi } from "./UpdateHint";

const open = vi.fn();
vi.mock("../chat/openLink", () => ({
  openInBrowser: (url: string) => {
    open(url);
  },
}));

beforeAll(async () => {
  await applyLanguage("en");
});

function fakeApi(portable = false) {
  let handler: ((info: UpdateInfo) => void) | null = null;
  const api: UpdateHintApi = {
    onUpdate: (h) => {
      handler = h;
      return () => {
        handler = null;
      };
    },
    isPortable: () => Promise.resolve(portable),
  };
  return {
    api,
    emit: (info: UpdateInfo) => {
      act(() => {
        handler?.(info);
      });
    },
  };
}

describe("UpdateHint", () => {
  it("shows nothing until an update is announced", () => {
    const { api } = fakeApi();
    render(<UpdateHint api={api} />);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("shows the version, links to the release and can be dismissed", async () => {
    const { api, emit } = fakeApi();
    render(<UpdateHint api={api} />);
    emit({ version: "0.5.0", url: "https://github.com/Mumcord/mumcord/releases/tag/v0.5.0" });
    const hint = await screen.findByRole("status");
    expect(hint).toHaveTextContent("Mumcord 0.5.0 is available.");
    expect(await axeViolations(hint)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "View release" }));
    expect(open).toHaveBeenCalledWith("https://github.com/Mumcord/mumcord/releases/tag/v0.5.0");
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("asks the portable build for the new portable zip", async () => {
    const { api, emit } = fakeApi(true);
    render(<UpdateHint api={api} />);
    emit({ version: "0.5.0", url: "https://github.com/Mumcord/mumcord/releases/tag/v0.5.0" });
    expect(await screen.findByText(/Download the new portable zip/)).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Mumcord 0.5.0 is available.");
  });

  it("keeps the plain hint for installed builds", async () => {
    const { api, emit } = fakeApi(false);
    render(<UpdateHint api={api} />);
    emit({ version: "0.5.0", url: "https://github.com/Mumcord/mumcord/releases/tag/v0.5.0" });
    const hint = await screen.findByRole("status");
    expect(hint).not.toHaveTextContent("portable");
  });
});
