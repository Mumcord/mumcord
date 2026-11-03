import { describe, expect, it } from "vitest";
import {
  endsWhen,
  fileTypeKey,
  folderOf,
  formatBytes,
  formatDuration,
  maxSizeLabel,
  progressParts,
  shortHash,
} from "./fileFormat";

describe("file card formatting", () => {
  it("formats sizes in 1024 steps like the plain text line", () => {
    expect(formatBytes(512, "en")).toBe("512 B");
    expect(formatBytes(19_293_798, "en")).toBe("18.4 MB");
    expect(formatBytes(1_395_864_371, "en")).toBe("1.3 GB");
  });

  it("builds the progress line parts in the unit of the file size", () => {
    const parts = progressParts(11_953_766, 19_293_798, 5_033_165, "en");
    expect(parts).toEqual({
      percent: 61,
      received: "11.4",
      size: "18.4 MB",
      speed: "4.8 MB/s",
      left: "2 s",
    });
    expect(progressParts(0, 100, 0, "en").left).toBeNull();
  });

  it("rounds the time left up to seconds, minutes or hours", () => {
    expect(formatDuration(0.2, "en")).toBe("1 s");
    expect(formatDuration(61, "en")).toBe("2 min");
    expect(formatDuration(3601, "en")).toBe("2 h");
  });

  it("labels the size limit setting", () => {
    expect(maxSizeLabel("500M", "en")).toBe("500 MB");
    expect(maxSizeLabel("2G", "en")).toBe("2 GB");
  });

  it("picks the file type, and a program for every risky file", () => {
    expect(fileTypeKey("slides.pdf", "application/pdf", false)).toBe("pdf");
    expect(fileTypeKey("raid.mkv", "video/x-matroska", false)).toBe("video");
    expect(fileTypeKey("notes", "application/octet-stream", false)).toBe("other");
    expect(fileTypeKey("setup.exe", "application/octet-stream", true)).toBe("windowsProgram");
    expect(fileTypeKey("run.sh", "text/plain", true)).toBe("script");
    expect(fileTypeKey("report.pdf.app", "application/pdf", true)).toBe("program");
  });

  it("shortens the checksum to its first and last four characters", () => {
    expect(shortHash("3f9a1c7e5b2d8046a9e3c1f7d5b8a24e60c9f1d3b7a5e8c2406d9b1f3a7ec21e")).toBe(
      "3f9a…c21e",
    );
  });

  it("says today or tomorrow for the share's end", () => {
    const now = new Date(2026, 10, 3, 20, 4);
    const tomorrow = new Date(2026, 10, 4, 20, 4).getTime() / 1000;
    expect(endsWhen(tomorrow, now, "en").key).toBe("tomorrow");
    expect(endsWhen(now.getTime() / 1000 + 60, now, "en").key).toBe("today");
  });

  it("finds the folder of a saved file on any system", () => {
    expect(folderOf("/home/bob/Downloads/slides.pdf")).toBe("/home/bob/Downloads");
    expect(folderOf("C:\\Users\\bob\\Downloads\\slides.pdf")).toBe("C:\\Users\\bob\\Downloads");
  });
});
