/**
 * Plugin sections on a profile card (FR-6.40 profile-card). Opening the card tells every
 * running plugin that may add a section (`profile-card-opened`); a plugin has 500 ms to answer
 * with a tree, otherwise its section is skipped. A section that was shown keeps following its
 * plugin's updates (a pat changes the pet's mood while the card is open).
 */
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type PluginNode } from "../../../api";
import { PluginTree } from "../render/PluginTree";
import { slotKey, usePluginUi } from "../render/pluginUiStore";
import { usePluginList } from "./badgeStore";

/** How long a plugin has to answer `profile-card-opened`. */
export const PROFILE_CARD_TIMEOUT_MS = 500;

export function PluginProfileSections({ session }: { session: number }) {
  // A new person is a new card: start with fresh answers and a fresh 500 ms window.
  return <Sections key={session} session={session} />;
}

function Sections({ session }: { session: number }) {
  const { t } = useTranslation("pluginBadges");
  const plugins = usePluginList();
  const { trees } = usePluginUi();
  const key = slotKey({ kind: "profileCard", session });
  const [openedAt] = useState(() => performance.now());
  /** The trees that were already there before the card asked; they may be stale. */
  const before = useRef<Map<string, readonly PluginNode[] | undefined>>(new Map());
  const [accepted, setAccepted] = useState<ReadonlySet<string>>(new Set());

  const askIds = plugins.map((p) => p.id).join("\n");
  useEffect(() => {
    const api = getApi();
    for (const id of askIds === "" ? [] : askIds.split("\n")) {
      before.current.set(id, trees.get(id)?.get(key));
      api.pluginUiEvent(id, { type: "profileCardOpened", session }).catch((error: unknown) => {
        console.error("Telling a plugin about the profile card failed", error);
      });
    }
    // Only a new card (or a changed set of plugins) asks again, not every tree update.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [askIds, session]);

  useEffect(() => {
    const onTime = performance.now() - openedAt <= PROFILE_CARD_TIMEOUT_MS;
    if (!onTime) return;
    const fresh = plugins
      .map((p) => p.id)
      .filter((id) => {
        const nodes = trees.get(id)?.get(key);
        return nodes !== undefined && nodes !== before.current.get(id) && !accepted.has(id);
      });
    if (fresh.length > 0) setAccepted(new Set([...accepted, ...fresh]));
  }, [trees, plugins, key, accepted, openedAt]);

  const sections = plugins.filter((p) => accepted.has(p.id));
  if (sections.length === 0) return null;
  return (
    <>
      {sections.map((p) => {
        const nodes = trees.get(p.id)?.get(key);
        if (nodes === undefined || nodes.length === 0) return null;
        return (
          <section
            key={p.id}
            className="plugin-profile-section"
            aria-label={t("section", { plugin: p.name })}
          >
            <PluginTree plugin={p.id} slot={{ kind: "profileCard", session }} nodes={nodes} />
          </section>
        );
      })}
    </>
  );
}
