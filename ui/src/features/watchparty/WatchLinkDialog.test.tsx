import { fireEvent, render, screen } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { stubDialog } from "../channels/testUtils";
import { WatchLinkDialog } from "./WatchLinkDialog";
import { startWatchParty } from "./startParty";

vi.mock("./startParty", () => ({ startWatchParty: vi.fn() }));
vi.mock("../channels", () => ({
  useServerStateOf: () => ({ users: new Map(), selfSession: null, connected: true }),
}));

beforeAll(async () => {
  stubDialog();
  await applyLanguage("en");
});

beforeEach(() => {
  setApi(createMockApi());
  vi.mocked(startWatchParty).mockClear();
});

describe("WatchLinkDialog inside the composer form", () => {
  it("starts the party without submitting the composer", () => {
    const composerSubmit = vi.fn((event: { preventDefault: () => void }) => {
      event.preventDefault();
    });
    const onClose = vi.fn();
    render(
      <form onSubmit={composerSubmit}>
        <WatchLinkDialog serverId={1} onClose={onClose} />
      </form>,
    );
    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "https://youtu.be/dQw4w9WgXcQ" },
    });
    fireEvent.submit(screen.getByRole("textbox").closest("form") as HTMLFormElement);
    expect(startWatchParty).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalled();
    expect(composerSubmit).not.toHaveBeenCalled();
  });
});
