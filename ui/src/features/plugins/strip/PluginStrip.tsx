import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { getApi, type PluginPanelVisibility, type PluginSlot } from "../../../api";
import { useT } from "../../../i18n/useT";
import { PluginTree } from "../render/PluginTree";
import { usePluginUi } from "../render/pluginUiStore";
import { usePluginIcon } from "../render/usePluginIcon";
import { useSharedSupported } from "./useSharedSupported";
import { usePlugins } from "../pluginStore";
import { PluginWebView, usePluginWebView } from "../webview";
import { loadOpen, saveOpen } from "./stripMemory";
import "./strip.css";

const PANEL: PluginSlot = { kind: "panel" };

function initials(name: string): string {
  return Array.from(name.trim()).slice(0, 2).join("").toUpperCase();
}

/** The side sheet shows the plugin's web view when it opened one there, else its panel. */
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
 * Plugins strip under the channel tree (FR-6.64): one icon per enabled plugin that has a panel.
 * An icon toggles that plugin's inline (mini) view below the row; its "Expand" button opens the
 * side sheet on the right.
 */
export function PluginStrip() {
  const t = useT("pluginStrip");
  const { overview } = usePlugins();
  const { trees, popoverRequest } = usePluginUi();
  const sharedSupported = useSharedSupported();
  const [openId, setOpenId] = useState(loadOpen);
  const [focusId, setFocusId] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const iconRefs = useRef(new Map<string, HTMLButtonElement>());
  const expandRef = useRef<HTMLButtonElement>(null);
  const reportedRef = useRef(new Map<string, PluginPanelVisibility>());
  const baseId = useId();

  const withPanel = (overview?.plugins ?? [])
    .filter((p) => p.enabled && p.status.kind === "running" && trees.get(p.id)?.has("panel"))
    .map((p) => ({ id: p.id, name: p.name }));
  const active = withPanel.find((p) => p.id === openId);
  const activeId = active?.id;
  const inline = activeId !== undefined;

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
        setOpenId(popoverRequest.plugin);
        saveOpen(popoverRequest.plugin);
        setExpanded(true);
      }
    } else if (popoverRequest.plugin === activeId) {
      setExpanded(false);
    }
  }

  const popover = activeId !== undefined && expanded;
  const closeRef = useRef<HTMLButtonElement>(null);
  const restoreFocusRef = useRef(false);
  useEffect(() => {
    if (popover) {
      closeRef.current?.focus();
      return;
    }
    if (!restoreFocusRef.current) return;
    restoreFocusRef.current = false;
    // The inline panel is not rendered while the sheet is open, so its expand button is new
    // after closing.
    expandRef.current?.focus();
  }, [popover]);

  if (withPanel.length === 0) return null;

  function toggleIcon(id: string) {
    const next = id === openId ? null : id;
    setOpenId(next);
    saveOpen(next);
    setExpanded(false);
  }

  function closePopover() {
    restoreFocusRef.current = true;
    setExpanded(false);
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
    setFocusId(next.id);
    iconRefs.current.get(next.id)?.focus();
  }

  function onPopoverKeys(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.stopPropagation();
    closePopover();
  }

  const panelId = `${baseId}-panel`;
  const tree = active && trees.get(active.id)?.get("panel");
  const body = active && tree && <PluginTree plugin={active.id} slot={PANEL} nodes={tree} />;

  return (
    <section
      className="pstrip"
      aria-label={t("heading")}
      data-tooltip={sharedSupported ? undefined : t("unsupported")}
    >
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
            aria-pressed={p.id === activeId}
            aria-controls={panelId}
            data-tooltip={p.name}
            tabIndex={p.id === (focusId ?? activeId ?? withPanel[0]?.id) ? 0 : -1}
            onFocus={() => {
              setFocusId(p.id);
            }}
            onClick={() => {
              toggleIcon(p.id);
            }}
            onKeyDown={(e) => {
              onIconKeys(e, index);
            }}
          >
            <PluginIcon id={p.id} name={p.name} />
          </button>
        ))}
      </div>
      <div id={panelId} hidden={!inline || expanded}>
        {active && inline && !expanded && (
          <div className="pstrip-panel" data-shared={sharedSupported ? "on" : "off"}>
            <div className="pstrip-panel-tools">
              <button
                type="button"
                ref={expandRef}
                className="pstrip-expand"
                aria-label={t("expand", { name: active.name })}
                data-tooltip={t("expand", { name: active.name })}
                aria-haspopup="dialog"
                aria-expanded={false}
                onClick={() => {
                  setExpanded(true);
                }}
              >
                <span aria-hidden="true">⤢</span>
              </button>
            </div>
            {!sharedSupported && <p className="pstrip-hint">{t("unsupported")}</p>}
            <div className="pstrip-body">{body}</div>
          </div>
        )}
      </div>
      {popover && active && (
        // Escape closes the dialog wherever focus is inside it.
        // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions
        <div
          className="pstrip-sheet"
          role="dialog"
          aria-label={active.name}
          onKeyDown={onPopoverKeys}
        >
          <div className="pstrip-panel-head">
            <span className="pstrip-panel-name">
              <PluginIcon id={active.id} name={active.name} />
              {active.name}
            </span>
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
