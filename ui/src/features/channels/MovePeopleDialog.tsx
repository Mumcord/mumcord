/**
 * "Move people to a channel" picker (Epic 4, issue #255): choose where the people should go from a
 * filterable list of the server's channels that shows each channel's user limit, and the button
 * confirms in plain words ("Move 3 people from Raid to Lobby?"). The server enforces the move
 * right and the user limit; a refusal arrives as the usual notice, so a full channel is named once
 * for the whole group instead of once per person.
 */
import { useEffect, useId, useMemo, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { SearchableSelect } from "../../components/SearchableSelect";
import type { SearchableOption } from "../../components/searchableOptions";
import { channelPath } from "../channelheader/channelModel";
import { Dialog } from "../servers/Dialog";
import {
  closeMovePeople,
  currentMovePeople,
  massMoveRunning,
  movePeopleToChannel,
  showMoveAlreadyRunning,
  showMoveResult,
  subscribeMovePeople,
  type MovePeopleRequest,
} from "./movePeople";
import { clearPeopleSelection } from "./peopleSelection";
import { useServerState } from "./serverStore";
import "./channels.css";

/** Renders the open picker; mount once, next to the other window-wide hosts. */
export function MovePeopleDialogHost() {
  const request = useSyncExternalStore(subscribeMovePeople, currentMovePeople);
  // Nothing may stay open once the host is gone.
  useEffect(() => closeMovePeople, []);
  return request === null ? null : <MovePeopleDialog request={request} />;
}

const collator = new Intl.Collator(undefined, { sensitivity: "base" });

function MovePeopleDialog({ request }: { request: MovePeopleRequest }) {
  const { t } = useTranslation("channels");
  const { t: tCommon } = useTranslation("common");
  const state = useServerState();
  const pickId = useId();
  const questionId = useId();
  const [target, setTarget] = useState("");
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);

  const sessions = request.sessions;
  const from =
    request.fromChannelId === null ? undefined : state.channels.get(request.fromChannelId);
  const targetChannel = target === "" ? undefined : state.channels.get(Number(target));
  const people = t("tree.people", { count: sessions.length });

  // People per channel, for the user limit next to each name (FR-3.85).
  const taken = useMemo(() => {
    const counts = new Map<number, number>();
    for (const user of state.users.values()) {
      counts.set(user.channelId, (counts.get(user.channelId) ?? 0) + 1);
    }
    return counts;
  }, [state.users]);

  // A channel everyone already sits in is no target; the rest come in tree order.
  const options = useMemo<SearchableOption[]>(() => {
    const byId = [...state.channels.values()].filter(
      (channel) => !sessions.every((session) => state.users.get(session)?.channelId === channel.id),
    );
    const pathOf = (id: number) => channelPath(state.channels, id).join(" / ");
    return byId
      .sort((a, b) => collator.compare(pathOf(a.id), pathOf(b.id)))
      .map((channel) => {
        const count = taken.get(channel.id) ?? 0;
        const limit =
          channel.maxUsers === null ? "" : ` (${String(count)}/${String(channel.maxUsers)})`;
        const path = pathOf(channel.id);
        return {
          value: String(channel.id),
          label: `${channel.name}${limit}`,
          keywords: path === channel.name ? [] : [path],
        };
      });
  }, [state.channels, state.users, sessions, taken]);

  const question =
    targetChannel === undefined
      ? null
      : from === undefined
        ? t("moveTo.questionNoSource", { people, to: targetChannel.name })
        : t("moveTo.question", { people, from: from.name, to: targetChannel.name });

  const close = () => {
    closeMovePeople();
  };

  const move = async () => {
    const serverId = state.serverId;
    if (serverId === null || targetChannel === undefined) return;
    // A second mass move would overtake the first one's counting (FR-10.18).
    if (massMoveRunning(serverId)) {
      showMoveAlreadyRunning(t, serverId, targetChannel.id);
      closeMovePeople();
      return;
    }
    setBusy(true);
    setFailed(false);
    const result = await movePeopleToChannel(serverId, sessions, targetChannel.id);
    setBusy(false);
    // Nothing left the engine: the dialog stays open and says so (FR-10.59).
    if (result.rejected === result.total) {
      setFailed(true);
      return;
    }
    clearPeopleSelection();
    closeMovePeople();
    showMoveResult(t, serverId, targetChannel.id, result);
  };

  return (
    <Dialog
      title={t("moveTo.title")}
      onClose={close}
      closeLabel={tCommon("close")}
      describedBy={question === null ? undefined : questionId}
    >
      <p className="move-people__hint">
        {from === undefined ? t("moveTo.hint") : t("moveTo.from", { channel: from.name })}
      </p>
      <label className="move-people__label" htmlFor={pickId}>
        {t("moveTo.channel")}
      </label>
      <SearchableSelect
        id={pickId}
        value={target}
        options={options}
        onChange={setTarget}
        placeholder={t("moveTo.search")}
        noMatch={t("moveTo.noMatch")}
        className="move-people__select"
      />
      {/* The question describes the dialog, so a screen reader hears what the button will do. */}
      <p id={questionId} className="move-people__question" aria-live="polite">
        {question}
      </p>
      {failed && (
        <p role="alert" className="move-people__error">
          {t("moveTo.failed")}
        </p>
      )}
      <div className="move-people__footer">
        <button type="button" className="button" onClick={close}>
          {tCommon("cancel")}
        </button>
        <button
          type="button"
          className="button button--primary"
          disabled={busy || targetChannel === undefined}
          onClick={() => {
            void move();
          }}
        >
          {t("moveTo.confirm")}
        </button>
      </div>
    </Dialog>
  );
}
