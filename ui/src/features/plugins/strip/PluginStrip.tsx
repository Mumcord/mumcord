import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { getApi, type PluginPanelVisibility, type PluginSlot } from "../../../api";
import { useT } from "../../../i18n/useT";
import { PluginTree } from "../render/PluginTree";
import { usePluginUi } from "../render/pluginUiStore";
import { usePluginIcon } from "../render/usePluginIcon";
import { useSharedSupported } from "./useSharedSupported";
import { usePlugins } from "../pluginStore";
import { PluginWebView, usePluginWebView } from "../webview";
import { loadCollapsed, saveCollapsed } from "./stripMemory";
import "./strip.css";

const PANEL: PluginSlot = { kind: "panel" };

function initials(name: string): string {
  return Array.from(name.trim()).slice(0, 2).join("").toUpperCase();
}

/** The expanded popover shows the plugin's web view when it opened one there, else its panel. */
function PopoverBody({ id, name, children }: { id: string; name: string; children: ReactNode }) {
  const webview = usePluginWebView(id, "panelPopover");
  return (
    <div className="pstrip-popover-body">
      {webview ? <PluginWebView plugin={id} webview={webview} title={name} /> : children}
    </div>
  );
}

function PluginIcon({ id, name }: { id: string; name: string }) {
  const url = usePluginIcon(id);
  return url === null ? (
    <span aria-hidden="true">{initials(name)}</span>
  ) : (
    <img src={url} alt="" width={20} height={20} />
  );
}

/**
 * Plugins strip under the channel tree (FR-6.64): one icon per enabled plugin that has a panel,
 * the active plugin's panel below, and an "Expand" button that opens the large popover.
 */
export function PluginStrip() {
  const t = useT("pluginStrip");
  const { overview } = usePlugins();
  const { trees, popoverRequest } = usePluginUi();
  const sharedSupported = useSharedSupported();
  const [collapsed, setCollapsed] = useState(loadCollapsed);
  const [chosen, setChosen] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const iconRefs = useRef(new Map<string, HTMLButtonElement>());
  const expandRef = useRef<HTMLButtonElement>(null);
  const reportedRef = useRef(new Map<string, PluginPanelVisibility>());
  const baseId = useId();

  const withPanel = (overview?.plugins ?? [])
    .filter((p) => p.enabled && p.status.kind === "running" && trees.get(p.id)?.has("panel"))
    .map((p) => ({ id: p.id, name: p.name }));
  const active = withPanel.find((p) => p.id === chosen) ?? withPanel[0];
  const activeId = active?.id;
  const inline = !collapsed && activeId !== undefined;

  // Tell each plugin whether its panel is visible (B18 contract); only changes are sent.
  const wanted = new Map<string, PluginPanelVisibility>();
  for (const p of withPanel) {
    const shown = p.id === activeId && (expanded || inline);
    wanted.set(p.id, shown ? (expanded ? "expanded" : "inline") : "hidden");
  }
  const wantedKey = [...wanted].map(([id, v]) => `${id}=${v}`).join("|");
  useEffect(() => {
    const reported = reportedRef.current;
    for (const [id, visibility] of wanted) {
      if (reported.get(id) === visibility) continue;
      reported.set(id, visibility);
      getApi()
        .pluginUiEvent(id, { type: "panelVisibility", visibility })
        .catch((error: unknown) => {
          console.error("Reporting the panel visibility failed", error);
        });
    }
    for (const id of [...reported.keys()]) if (!wanted.has(id)) reported.delete(id);
    // `wantedKey` stands for the contents of `wanted`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wantedKey]);

  // A plugin opened or closed its own popover (the host only forwards that after a user gesture).
  // Adjusting state while rendering, keyed on the request number, avoids an extra effect pass.
  const [handledPopover, setHandledPopover] = useState(popoverRequest?.seq ?? 0);
  if (popoverRequest != null && popoverRequest.seq !== handledPopover) {
    setHandledPopover(popoverRequest.seq);
    if (popoverRequest.open) {
      if (withPanel.some((p) => p.id === popoverRequest.plugin)) {
        setChosen(popoverRequest.plugin);
        setExpanded(true);
      }
    } else if (popoverRequest.plugin === activeId) {
      setExpanded(false);
    }
  }

  const popover = activeId !== undefined && expanded;
  const closeRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (popover) closeRef.current?.focus();
  }, [popover]);

  if (withPanel.length === 0 || active === undefined) return null;

  function toggleCollapsed() {
    const next = !collapsed;
    setCollapsed(next);
    saveCollapsed(next);
    setExpanded(false);
  }

  function closePopover() {
    setExpanded(false);
    // The inline panel (and its expand button) is hidden while collapsed; go back to the icon.
    const target = collapsed ? iconRefs.current.get(activeId ?? "") : expandRef.current;
    target?.focus();
  }

  function onIconKeys(event: KeyboardEvent, index: number) {
    const last = withPanel.length - 1;
    const target =
      event.key === "ArrowRight" || event.key === "ArrowDown"
        ? (index + 1) % (last + 1)
        : event.key === "ArrowLeft" || event.key === "ArrowUp"
          ? (index + last) % (last + 1)
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? last
              : null;
    if (target === null) return;
    event.preventDefault();
    const next = withPanel[target];
    if (next === undefined) return;
    setChosen(next.id);
    iconRefs.current.get(next.id)?.focus();
  }

  function onPopoverKeys(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.stopPropagation();
    closePopover();
  }

  const panelId = `${baseId}-panel`;
  const tree = trees.get(active.id)?.get("panel");
  const body = tree && <PluginTree plugin={active.id} slot={PANEL} nodes={tree} />;

  return (
    <section
      className="pstrip"
      aria-label={t("heading")}
      data-tooltip={sharedSupported ? undefined : t("unsupported")}
    >
      <div className="pstrip-bar">
        <button
          type="button"
          className="pstrip-toggle"
          aria-expanded={!collapsed}
          aria-controls={panelId}
          data-tooltip={collapsed ? t("show") : t("hide")}
          onClick={toggleCollapsed}
        >
          <span aria-hidden="true">{collapsed ? "▸" : "▾"}</span> {t("heading")}
        </button>
        <div className="pstrip-icons" role="toolbar" aria-label={t("list")}>
          {withPanel.map((p, index) => (
            <button
              key={p.id}
              type="button"
              ref={(el) => {
                if (el) iconRefs.current.set(p.id, el);
                else iconRefs.current.delete(p.id);
              }}
              className="pstrip-icon"
              aria-label={p.name}
              aria-pressed={collapsed ? undefined : p.id === active.id}
              aria-haspopup={collapsed ? "dialog" : undefined}
              aria-expanded={collapsed ? expanded && p.id === active.id : undefined}
              data-tooltip={p.name}
              tabIndex={p.id === active.id ? 0 : -1}
              onClick={() => {
                setChosen(p.id);
                // Collapsed, an icon has no inline panel to show, so it opens the large popover.
                if (collapsed) setExpanded(true);
              }}
              onKeyDown={(e) => {
                onIconKeys(e, index);
              }}
            >
              <PluginIcon id={p.id} name={p.name} />
            </button>
          ))}
        </div>
      </div>
      <div id={panelId} hidden={collapsed}>
        {inline && (
          <div className="pstrip-panel" data-shared={sharedSupported ? "on" : "off"}>
            <div className="pstrip-panel-head">
              <span className="pstrip-panel-name">{active.name}</span>
              <button
                type="button"
                ref={expandRef}
                className="pstrip-expand"
                aria-label={t("expand", { name: active.name })}
                data-tooltip={t("expand", { name: active.name })}
                aria-haspopup="dialog"
                aria-expanded={expanded}
                onClick={() => {
                  setExpanded(true);
                }}
              >
                <span aria-hidden="true">⤢</span>
              </button>
            </div>
            {!sharedSupported && <p className="pstrip-hint">{t("unsupported")}</p>}
            {!expanded && <div className="pstrip-body">{body}</div>}
          </div>
        )}
      </div>
      {popover && (
        // Escape closes the dialog wherever focus is inside it.
        // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions
        <div
          className="pstrip-popover"
          role="dialog"
          aria-label={active.name}
          onKeyDown={onPopoverKeys}
        >
          <div className="pstrip-panel-head">
            <span className="pstrip-panel-name">{active.name}</span>
            <button
              type="button"
              className="pstrip-expand"
              ref={closeRef}
              aria-label={t("close")}
              data-tooltip={t("close")}
              onClick={closePopover}
            >
              <span aria-hidden="true">✕</span>
            </button>
          </div>
          <PopoverBody id={active.id} name={active.name}>
            {body}
          </PopoverBody>
        </div>
      )}
    </section>
  );
}
