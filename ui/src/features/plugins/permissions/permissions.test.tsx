import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import {
  defaultOptionalSelection,
  PERMISSION_RISK,
  permissionTextKey,
  type PermissionId,
  type RequestedPermission,
} from "./catalogue";
import { PermissionList } from "./PermissionList";
import en from "../../../locales/en/pluginPermissions.json";

beforeEach(async () => {
  await applyLanguage("en");
});

describe("permission catalogue", () => {
  const rust = readFileSync(
    resolve(import.meta.dirname, "../../../../../crates/plugins/src/permissions.rs"),
    "utf8",
  );
  const rustEntries = [
    ...rust.matchAll(/\b(?:all|official)\(\s*"([^"]+)",\s*Risk::(Green|Yellow|Red)/g),
  ].map((m) => [m[1], (m[2] ?? "").toLowerCase()]);

  it("matches the Rust catalogue (ids and risks)", () => {
    expect(rustEntries).toHaveLength(19);
    expect(Object.fromEntries(rustEntries)).toEqual(PERMISSION_RISK);
  });

  it("has an English text for every id", () => {
    const ids = Object.keys(PERMISSION_RISK) as PermissionId[];
    expect(Object.keys(en.texts).sort()).toEqual(ids.map(permissionTextKey).sort());
  });
});

describe("PermissionList", () => {
  const requested: RequestedPermission[] = [
    { id: "ui.chat-card", required: true },
    { id: "net.http", required: true },
    { id: "ui.panel", required: false, suggested: true },
    { id: "notify", required: false },
  ];

  it("starts with only the suggested optional permissions on", () => {
    expect(defaultOptionalSelection(requested)).toEqual(new Set(["ui.panel"]));
  });

  it("shows required rows fixed, optional rows as toggles and red as High risk text", () => {
    render(
      <PermissionList requested={requested} enabled={new Set(["ui.panel"])} onToggle={vi.fn()} />,
    );
    expect(screen.getByText("Show cards in the chat")).toBeTruthy();
    expect(screen.getByText("High risk")).toBeTruthy();
    const boxes = screen.getAllByRole("checkbox");
    expect(boxes).toHaveLength(2);
    expect((boxes[0] as HTMLInputElement).checked).toBe(true);
    expect((boxes[1] as HTMLInputElement).checked).toBe(false);
  });

  it("reports toggles", () => {
    const onToggle = vi.fn();
    render(<PermissionList requested={requested} enabled={new Set()} onToggle={onToggle} />);
    fireEvent.click(screen.getByRole("checkbox", { name: /Show notifications/ }));
    expect(onToggle).toHaveBeenCalledWith("notify", true);
  });

  it("disables toggles when read-only", () => {
    render(
      <PermissionList requested={requested} enabled={new Set()} onToggle={vi.fn()} readOnly />,
    );
    for (const box of screen.getAllByRole("checkbox")) {
      expect((box as HTMLInputElement).disabled).toBe(true);
    }
  });

  it("has no accessibility violations", async () => {
    const { container } = render(
      <PermissionList requested={requested} enabled={new Set()} onToggle={vi.fn()} />,
    );
    expect(await axeViolations(container)).toEqual([]);
  });
});
