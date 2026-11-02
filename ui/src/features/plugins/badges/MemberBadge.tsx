/**
 * The plugin badge next to a person's name (FR-6.40 member-badge, FR-6.63) and the 16 px chat
 * decoration. Only the plugin chosen in Settings → Plugins may show one, so a person never has
 * more than one badge (Q2.7).
 */
import { memo } from "react";
import { useTranslation } from "react-i18next";
import type { PluginNode, PluginSlot } from "../../../api";
import { PluginTree } from "../render/PluginTree";
import { usePluginTree } from "../render/pluginUiStore";
import { useBadgePlugin } from "./badgeStore";
import "./badges.css";

const Shown = memo(function ShownBadge({
  plugin,
  name,
  slot,
  nodes,
  className,
}: {
  plugin: string;
  name: string;
  slot: PluginSlot;
  nodes: readonly PluginNode[];
  className: string;
}) {
  const { t } = useTranslation("pluginBadges");
  return (
    <span className={className} role="group" aria-label={t("label", { plugin: name })}>
      <PluginTree plugin={plugin} slot={slot} nodes={nodes} />
    </span>
  );
});

function SlotBadge({
  session,
  kind,
  className,
}: {
  session: number;
  kind: "memberBadge" | "chatDecoration";
  className: string;
}) {
  const badge = useBadgePlugin();
  const slot: PluginSlot = { kind, session };
  const nodes = usePluginTree(badge?.id ?? "", slot);
  if (badge === null || nodes === undefined || nodes.length === 0) return null;
  return (
    <Shown plugin={badge.id} name={badge.name} slot={slot} nodes={nodes} className={className} />
  );
}

/** Badge next to a name in the channel tree and member lists. */
export function MemberBadge({ session }: { session: number }) {
  return <SlotBadge session={session} kind="memberBadge" className="plugin-badge" />;
}

/** Decoration next to a sender's name in chat. */
export function ChatDecoration({ session }: { session: number }) {
  return <SlotBadge session={session} kind="chatDecoration" className="plugin-decoration" />;
}
