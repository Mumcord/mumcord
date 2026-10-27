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

function fakeApi() {
  let handler: ((info: UpdateInfo) => void) | null = null;
  const api: UpdateHintApi = {
    onUpdate: (h) => {
      handler = h;
      return () => {
        handler = null;
      };
    },
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
});
