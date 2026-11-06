import { render, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it } from "vitest";
import type { RefusedPlugin } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { RefusedPlugins } from "./RefusedPlugins";

const badLabel: RefusedPlugin = {
  name: "test-badlabel",
  issue: { kind: "refused_app_menu_label" },
  atMillis: 0,
};
const noPermission: RefusedPlugin = {
  name: "test-field",
  issue: { kind: "field_without_permission", field: "net", permission: "net.origins" },
  atMillis: 0,
};

beforeAll(async () => {
  await applyLanguage("en");
});

describe("RefusedPlugins", () => {
  it("shows nothing when no plugin was refused", () => {
    const { container } = render(<RefusedPlugins refused={[]} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("lists each refused plugin with its reason", async () => {
    const { container } = render(<RefusedPlugins refused={[badLabel, noPermission]} />);
    expect(screen.getByRole("heading", { name: "Refused plugins" })).toBeTruthy();
    expect(screen.getByText("test-badlabel")).toBeTruthy();
    expect(screen.getByText(/an Apps-menu label is the same as one of Mumcord's own/)).toBeTruthy();
    expect(screen.getByText(/uses "net" without the permission "net.origins"/)).toBeTruthy();
    expect(await axeViolations(container)).toEqual([]);
  });
});
