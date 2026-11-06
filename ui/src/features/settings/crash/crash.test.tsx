import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { createMockApi, setApi, type CrashReport } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { stubDialog } from "../../channels/testUtils";
import { CrashNotice } from "../../app/CrashNotice";
import { resetUnseenOnce } from "../../app/crashNoticeOnce";
import { CrashReportsCard } from "./CrashReportsCard";

const report = (): CrashReport => ({
  id: "crash-1794001864.txt",
  created: 1794001864,
  version: "0.7.1",
  kind: "signal",
  signal: "SIGSEGV",
  cause: "Mumcord was stopped by SIGSEGV",
  uploaded: false,
});

describe("crash reports (#568, #573)", () => {
  beforeEach(async () => {
    stubDialog();
    await applyLanguage("en");
  });

  it("shows an empty list without reports", async () => {
    setApi(createMockApi());
    render(<CrashReportsCard />);
    expect(await screen.findByText("No crash reports")).toBeTruthy();
  });

  it("uploads one report only after the confirmation, with the comment, and marks it", async () => {
    setApi(createMockApi({ crashReports: [report()] }));
    render(<CrashReportsCard />);
    fireEvent.click(await screen.findByRole("button", { name: /Report the crash from/ }));

    // The confirmation says plainly what is shared.
    expect(await screen.findByRole("note")).toHaveProperty(
      "textContent",
      expect.stringContaining("can identify this computer"),
    );
    fireEvent.change(screen.getByLabelText("What were you doing? (optional)"), {
      target: { value: "watching a VP9 share" },
    });
    await waitFor(() => {
      expect(screen.getByLabelText<HTMLTextAreaElement>("Report text").value).toContain(
        "comment:\nwatching a VP9 share",
      );
    });
    expect(screen.queryByText("Uploaded")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Send report" }));
    expect(await screen.findByText("Report sent. Thank you!")).toBeTruthy();
    expect(await screen.findByText("Uploaded")).toBeTruthy();
  });

  it("deletes a report after the confirmation", async () => {
    setApi(createMockApi({ crashReports: [report()] }));
    render(<CrashReportsCard />);
    fireEvent.click(await screen.findByRole("button", { name: /Delete report from/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    expect(await screen.findByText("No crash reports")).toBeTruthy();
  });

  it("shows the crash notice once, also when React runs effects twice", async () => {
    resetUnseenOnce();
    setApi(createMockApi({ crashReports: [report()] }));
    render(
      <StrictMode>
        <CrashNotice />
      </StrictMode>,
    );
    expect(await screen.findByText("Mumcord closed unexpectedly")).toBeTruthy();
  });
});
