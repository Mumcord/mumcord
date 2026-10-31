/** Channel column content: the channel tree of the connected server, or an empty state. */
import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { getApi, toEngineError } from "../../api";
import { pinPerson } from "../members/personClick";
import { useServers as useSavedServers } from "../servers";
import { displayName } from "../servers/format";
import { joinChannel, useReadOnly } from "../userpanel/readOnly";
import { useServerAnnouncements } from "./announcements";
import { BigServerGate } from "./BigServerGate";
import { ChannelSearch } from "./ChannelSearch";
import {
  ChannelTree,
  type ChannelAction,
  type ChannelMenuRequest,
  type ChannelTreeHandle,
} from "./ChannelTree";
import { JumpToChannelButton } from "./JumpToChannelButton";
import { ServerSectionHeader } from "./ServerSectionHeader";
import { useSelectedConversation } from "./selection";
import { selfUser, useServerState } from "./serverStore";
import { BIG_SERVER_CHANNELS } from "./tree";

const SEARCH_DEBOUNCE_MS = 100;

export interface ChannelsPanelProps {
  /** Extra content after a channel's people count, e.g. the unread badge (B22). */
  channelBadge?: ((channelId: number) => ReactNode) | undefined;
  /** Button in a fixed slot of each channel row, e.g. the follow star (QA.13). */
  channelAction?: ((channel: ChannelAction) => ReactNode) | undefined;
  /** The server's ⋯ menu in the section header above the tree (QA.13, issue #172). */
  header?: ReactNode;
  /** Entry between the search and the tree, e.g. "Message requests" (issue #141). */
  aboveTree?: ReactNode;
  /** A channel's right-click menu (issue #172). */
  channelMenu?: ((menu: ChannelMenuRequest) => ReactNode) | undefined;
}

export function ChannelsPanel({
  channelBadge,
  channelAction,
  header,
  aboveTree,
  channelMenu,
}: ChannelsPanelProps) {
  const { t } = useTranslation("channels");
  const readOnly = useReadOnly();
  const state = useServerState();
  const { conversation, select } = useSelectedConversation();
  const [joinError, setJoinError] = useState<string | null>(null);
  const treeRef = useRef<ChannelTreeHandle>(null);
  const treeId = useId();
  const { servers } = useSavedServers();
  // Collapsed server sections, by server id; switching servers keeps each one's state.
  const [collapsedServers, setCollapsedServers] = useState<ReadonlySet<number>>(new Set());
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

  const serverId = state.serverId;
  const movePerson = useCallback(
    (session: number, channelId: number) => {
      if (serverId === null) return;
      setJoinError(null);
      getApi()
        .personMove(serverId, session, channelId)
        .catch((cause: unknown) => {
          const { code } = toEngineError(cause);
          console.error("Moving a person failed", code);
          setJoinError(t("errors.moveFailed"));
        });
    },
    [serverId, t],
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

  const saved = servers.find((s) => s.id === state.serverId);
  const serverName = saved ? displayName(saved) : (state.info?.serverLabel ?? "");
  const collapsed = collapsedServers.has(state.serverId);

  return (
    <div className="channels-panel">
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
      {aboveTree}
      <ServerSectionHeader
        name={serverName}
        connection={state.connection.state}
        online={state.users.size}
        collapsed={collapsed}
        onToggle={() => {
          setCollapsedServers((ids) => {
            const next = new Set(ids);
            if (collapsed) next.delete(state.serverId ?? -1);
            else if (state.serverId !== null) next.add(state.serverId);
            return next;
          });
        }}
        controls={treeId}
        menu={header}
        end={
          <JumpToChannelButton
            disabled={readOnly || selfUser(state) === undefined}
            onJump={() => {
              // The own channel may be filtered out; jumping shows the whole tree again.
              clearSearch();
              if (collapsed)
                setCollapsedServers(
                  (ids) => new Set([...ids].filter((id) => id !== state.serverId)),
                );
              treeRef.current?.jumpToVoice();
            }}
          />
        }
      />
      <div id={treeId} hidden={collapsed}>
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
            channelMenu={channelMenu}
            onMovePerson={movePerson}
            filter={filter}
          />
        </BigServerGate>
      </div>
      {joinError && (
        <p role="alert" className="channels-error">
          {joinError}
        </p>
      )}
    </div>
  );
}
