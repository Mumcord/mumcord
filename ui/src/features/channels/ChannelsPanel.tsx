/** Channel column content: the channel tree of the connected server, or an empty state. */
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { toEngineError } from "../../api";
import { pinPerson } from "../members/personClick";
import { joinChannel, useReadOnly } from "../userpanel/readOnly";
import { useServerAnnouncements } from "./announcements";
import { BigServerGate } from "./BigServerGate";
import { ChannelSearch } from "./ChannelSearch";
import { ChannelTree, type ChannelAction, type ChannelTreeHandle } from "./ChannelTree";
import { JumpToChannelButton } from "./JumpToChannelButton";
import { useSelectedConversation } from "./selection";
import { selfUser, useServerState } from "./serverStore";
import { BIG_SERVER_CHANNELS } from "./tree";

const SEARCH_DEBOUNCE_MS = 100;

export interface ChannelsPanelProps {
  /** Extra content after a channel's people count, e.g. the unread badge (B22). */
  channelBadge?: ((channelId: number) => ReactNode) | undefined;
  /** Button in a fixed slot of each channel row, e.g. the follow star (QA.13). */
  channelAction?: ((channel: ChannelAction) => ReactNode) | undefined;
  /** Row above the tree, e.g. the server name with its menu (QA.13); "Jump to my channel" follows it. */
  header?: ReactNode;
}

export function ChannelsPanel({ channelBadge, channelAction, header }: ChannelsPanelProps) {
  const { t } = useTranslation("channels");
  const readOnly = useReadOnly();
  const state = useServerState();
  const { conversation, select } = useSelectedConversation();
  const [joinError, setJoinError] = useState<string | null>(null);
  const treeRef = useRef<ChannelTreeHandle>(null);
  // What the search field shows, and the query the tree filters by: the query follows ~100 ms
  // later, so typing quickly filters a big tree once instead of on every key (QA.27).
  const [searchText, setSearchText] = useState("");
  const [filter, setFilter] = useState("");
  useServerAnnouncements();

  useEffect(() => {
    if (searchText === filter) return;
    const timer = setTimeout(() => {
      setFilter(searchText);
    }, SEARCH_DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [searchText, filter]);

  const clearSearch = () => {
    setSearchText("");
    setFilter("");
  };

  useEffect(() => {
    if (import.meta.env.DEV) {
      void import("./devConnect").then((m) => {
        m.devAutoConnect();
      });
    }
  }, []);

  const join = useCallback(
    (channelId: number) => {
      setJoinError(null);
      joinChannel(channelId).catch((cause: unknown) => {
        const { code } = toEngineError(cause);
        if (code !== "permissionDenied" && code !== "notConnected") {
          console.error("Joining a channel failed", code);
        }
        setJoinError(
          code === "permissionDenied"
            ? t("errors.joinDenied")
            : code === "notConnected"
              ? t("errors.notConnected")
              : t("errors.joinFailed"),
        );
      });
    },
    [t],
  );

  if (state.serverId === null) {
    // A big server takes seconds to sync; say so as soon as its channel count shows it (AC-2.17).
    const syncingBig = state.syncChannels > BIG_SERVER_CHANNELS;
    return (
      <p className="channels-empty" role={syncingBig ? "status" : undefined}>
        {state.connection.state === "disconnected"
          ? t("empty.disconnected")
          : syncingBig
            ? t("empty.syncingBig")
            : t("empty.loading")}
      </p>
    );
  }

  return (
    <div className="channels-panel">
      <div className="channels-header">
        {header}
        <JumpToChannelButton
          disabled={readOnly || selfUser(state) === undefined}
          onJump={() => {
            // The own channel may be filtered out; jumping shows the whole tree again.
            clearSearch();
            treeRef.current?.jumpToVoice();
          }}
        />
      </div>
      <ChannelSearch
        value={searchText}
        onChange={setSearchText}
        onClear={clearSearch}
        onSubmit={() => {
          if (searchText === filter) {
            treeRef.current?.openFirstMatch();
            return;
          }
          // Apply the pending query now; the tree shows its matches after this event's render.
          setFilter(searchText);
          requestAnimationFrame(() => treeRef.current?.openFirstMatch());
        }}
      />
      <section className="server-overview" aria-labelledby="server-overview-title">
        <h2 id="server-overview-title">{t("serverOverview")}</h2>
      </section>
      <BigServerGate
        // A fresh tree per server, so its expand/collapse state starts from that server's (QA.27).
        key={state.serverId}
        big={state.channels.size > BIG_SERVER_CHANNELS}
        className="channels-empty"
      >
        <ChannelTree
          ref={treeRef}
          state={state}
          readOnly={readOnly}
          selectedChannelId={conversation?.kind === "channel" ? conversation.channelId : null}
          onSelect={(channelId) => {
            setJoinError(null);
            select({ kind: "channel", channelId });
          }}
          onJoin={join}
          onOpenPerson={(session, opener) => {
            if (opener) pinPerson(session, opener);
          }}
          channelBadge={channelBadge}
          channelAction={channelAction}
          filter={filter}
        />
      </BigServerGate>
      {joinError && (
        <p role="alert" className="channels-error">
          {joinError}
        </p>
      )}
    </div>
  );
}
