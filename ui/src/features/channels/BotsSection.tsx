/**
 * "Bots on this server" below the channel tree (issue #458): every bot that announced itself in
 * its hello, with what it does. Bots without voice are listed only here. All rows share one Tab
 * stop (Up/Down/Home/End) like the member sections; hover or focus shows the profile preview,
 * Enter or click pins it, and the context menu offers the same actions as for people.
 */
import { useRef, useState, type KeyboardEvent } from "react";
import type { User } from "../../api";
import { useT } from "../../i18n/useT";
import { Avatar } from "../members/Avatar";
import { CollapsibleSection } from "../members/CollapsibleSection";
import { clickPerson, doubleClickPerson } from "../members/personClick";
import { ProfilePreviewTrigger } from "../members/ProfilePreview";
import { useAvatars } from "../members/useAvatars";
import { useSectionOpen } from "../members/useSectionOpen";
import "../members/members.css";
import { BotBadge } from "./BotBadge";
import { BOT_MODULES, orderedModules, serverBots } from "./mumcord";
import type { ServerState } from "./serverStore";

export function BotsSection({ state }: { state: ServerState }) {
  const t = useT("channels");
  const [open, setOpen] = useSectionOpen("server-bots", true);
  const listRef = useRef<HTMLUListElement>(null);
  const [focused, setFocused] = useState<number | null>(null);
  const bots = serverBots(state.users);
  const avatarOf = useAvatars(open ? bots : [], state.serverId ?? undefined);
  if (bots.length === 0) return null;

  const tabStop = bots.find((bot) => bot.session === focused)?.session ?? bots[0]?.session;

  const describe = (bot: User): string => {
    const known = new Set(BOT_MODULES);
    const parts = orderedModules(bot.bot?.modules ?? []).map((module) =>
      known.has(module) ? t(`bots.modules.${module}`) : module,
    );
    const channel = state.channels.get(bot.channelId);
    if (bot.bot?.voice === true && channel) {
      parts.push(t("bots.inChannel", { channel: channel.name }));
    }
    return parts.join(" · ");
  };

  const onKeyDown = (event: KeyboardEvent<HTMLUListElement>) => {
    const index = bots.findIndex((bot) => bot.session === tabStop);
    const targets: Record<string, number> = {
      ArrowDown: index + 1,
      ArrowUp: index - 1,
      Home: 0,
      End: bots.length - 1,
    };
    const targetIndex = targets[event.key];
    if (targetIndex === undefined) return;
    event.preventDefault();
    const target = bots[targetIndex];
    if (target === undefined) return;
    setFocused(target.session);
    listRef.current
      ?.querySelector<HTMLElement>(`[data-session="${String(target.session)}"]`)
      ?.focus();
  };

  return (
    <CollapsibleSection
      id="server-bots"
      className="bots-section"
      title={t("bots.title", { total: bots.length })}
      open={open}
      onOpenChange={setOpen}
    >
      {() => (
        // The key handler moves focus between the buttons inside; it is not itself interactive.
        // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions
        <ul className="members-list" ref={listRef} onKeyDown={onKeyDown}>
          {bots.map((bot) => {
            const what = describe(bot);
            return (
              <li key={bot.session}>
                <ProfilePreviewTrigger session={bot.session}>
                  <button
                    type="button"
                    className="member bot-row"
                    data-session={bot.session}
                    tabIndex={bot.session === tabStop ? 0 : -1}
                    onFocus={() => {
                      setFocused(bot.session);
                    }}
                    onClick={(event) => {
                      clickPerson(event.detail, bot.session, event.currentTarget);
                    }}
                    onDoubleClick={() => {
                      doubleClickPerson(bot.session);
                    }}
                  >
                    <Avatar
                      name={bot.name}
                      src={avatarOf(bot)}
                      talking={state.talking.has(bot.session)}
                      size="sm"
                    />
                    <span className="bot-row__text">
                      <span className="bot-row__name">
                        {bot.name}
                        <BotBadge />
                      </span>
                      {what !== "" && (
                        <span className="bot-row__what">
                          {/* The space keeps "Bot," and the line separate words in the name. */}
                          <span className="visually-hidden">,</span> {what}
                        </span>
                      )}
                    </span>
                  </button>
                </ProfilePreviewTrigger>
              </li>
            );
          })}
        </ul>
      )}
    </CollapsibleSection>
  );
}
