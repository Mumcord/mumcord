/**
 * Renders a plugin's declarative tree (FR-6.40 … FR-6.42) with Mumcord's own components. The
 * host already refused trees that break the limits; the renderer still guards against bad
 * indices and cycles so a broken tree can never hang or crash the app.
 */
import { Fragment, useCallback, useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type PluginActionValue, type PluginNode, type PluginSlot } from "../../../api";
import { RenderCtx, type RenderContext } from "./context";
import {
  PluginButton,
  PluginCopyButton,
  PluginIconButton,
  PluginSelect,
  PluginSlider,
  PluginToggle,
} from "./controls";
import { PluginCountdown } from "./Countdown";
import {
  PluginChip,
  PluginDivider,
  PluginImage,
  PluginMeter,
  PluginProgress,
  PluginText,
} from "./display";
import { PluginFilePicker, PluginPassword, PluginTextInput } from "./inputs";
import { useReducedMotion } from "./motion";
import { PluginList } from "./SortableList";
import { PluginSprite } from "./Sprite";
import "./render.css";

export interface PluginTreeProps {
  plugin: string;
  slot: PluginSlot;
  nodes: readonly PluginNode[];
}

const JUSTIFY = { start: "flex-start", end: "flex-end", spaceBetween: "space-between" } as const;

/** Gap of a stack in px; the host refuses more. */
const MAX_GAP = 32;

function renderNode(
  nodes: readonly PluginNode[],
  index: number,
  seen: Set<number>,
  reducedMotion: boolean,
): ReactNode {
  const node = nodes[index];
  if (node === undefined || seen.has(index)) return null;
  seen.add(index);
  const children = () =>
    node.children.map((child) => (
      <Fragment key={child}>{renderNode(nodes, child, seen, reducedMotion)}</Fragment>
    ));
  const c = node.component;
  let content: ReactNode;
  switch (c.type) {
    case "stack":
      content = (
        <div
          className={`pr-stack pr-stack--${c.direction}`}
          style={{
            gap: Math.min(c.gap, MAX_GAP),
            alignItems: c.align === "start" || c.align === "end" ? `flex-${c.align}` : c.align,
            justifyContent: JUSTIFY[c.justify ?? "start"],
          }}
        >
          {children()}
        </div>
      );
      break;
    case "list":
      content = <PluginList list={c} rows={children()} />;
      break;
    case "text":
      content = <PluginText {...c} />;
      break;
    case "image":
      content = <PluginImage {...c} />;
      break;
    case "sprite":
      content = <PluginSprite {...c} />;
      break;
    case "meter":
      content = <PluginMeter {...c} />;
      break;
    case "button":
      content = <PluginButton {...c} />;
      break;
    case "copyButton":
      content = <PluginCopyButton {...c} />;
      break;
    case "iconButton":
      content = <PluginIconButton {...c} />;
      break;
    case "chip":
      content = <PluginChip {...c} />;
      break;
    case "divider":
      content = <PluginDivider />;
      break;
    case "progress":
      content = <PluginProgress {...c} />;
      break;
    case "toggle":
      content = <PluginToggle {...c} />;
      break;
    case "select":
      content = <PluginSelect {...c} />;
      break;
    case "slider":
      content = <PluginSlider {...c} />;
      break;
    case "textInput":
      content = <PluginTextInput {...c} />;
      break;
    case "password":
      content = <PluginPassword {...c} />;
      break;
    case "filePicker":
      content = <PluginFilePicker {...c} />;
      break;
    case "countdown":
      content = <PluginCountdown {...c} />;
      break;
  }
  if (node.animation === null || reducedMotion) return content;
  return <div className={`pr-anim pr-anim--${node.animation}`}>{content}</div>;
}

export function PluginTree({ plugin, slot, nodes }: PluginTreeProps) {
  const { t } = useTranslation("pluginSlots");
  const reducedMotion = useReducedMotion();
  const [failed, setFailed] = useState(false);

  const send = useCallback(
    (action: string, value: PluginActionValue) => {
      setFailed(false);
      getApi()
        .pluginUiAction(plugin, slot, action, value)
        .catch((error: unknown) => {
          console.error("A plugin action failed", error);
          setFailed(true);
        });
    },
    [plugin, slot],
  );

  const ctx = useMemo<RenderContext>(
    () => ({ plugin, send, reducedMotion }),
    [plugin, send, reducedMotion],
  );

  const root = nodes.length === 0 ? null : renderNode(nodes, 0, new Set(), reducedMotion);
  return (
    <RenderCtx value={ctx}>
      <div className="pr-root">
        {root ?? <span className="pr-alt">{t("tree.empty")}</span>}
        {failed && (
          <span className="pr-error" role="alert">
            {t("tree.actionFailed")}
          </span>
        )}
      </div>
    </RenderCtx>
  );
}
