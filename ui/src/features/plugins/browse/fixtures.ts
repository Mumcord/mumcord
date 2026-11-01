import type { IndexedPlugin, PluginIndex } from "../../../api";

/** A store entry for tests. */
export function indexed(id: string, overrides: Partial<IndexedPlugin> = {}): IndexedPlugin {
  return {
    id,
    source: { kind: "official" },
    name: id.charAt(0).toUpperCase() + id.slice(1),
    summary: `About ${id}`,
    descriptionMdUrl: null,
    icon: null,
    screenshots: [],
    categories: [],
    authors: ["Mumcord"],
    reviewed: true,
    updated: "2026-10-01",
    version: "1.0.0",
    size: 4096,
    permissions: [],
    ...overrides,
  };
}

export function index(plugins: IndexedPlugin[], overrides: Partial<PluginIndex> = {}): PluginIndex {
  return { freshness: "fresh", notice: null, plugins, ...overrides };
}
