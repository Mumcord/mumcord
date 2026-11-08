/**
 * The question a command asks before it acts (Epic 4, issue #378, FR-10.55, FR-10.56): `/unban`
 * shows the name, reason and end of the entry it would remove, `/moveall` how many people move
 * from where to where. The title carries the server the command belongs to (FR-10.58), the question
 * opens on its answer button so Enter confirms, Escape cancels, and the buttons are named after
 * what they do.
 */
import { useId, useRef } from "react";
import { useTranslation } from "react-i18next";
import { getServerStateOf } from "../../channels/serverStore";
import { Dialog } from "../../servers/Dialog";
import { displayName, initials } from "../../servers/format";
import { useServers } from "../../servers/store";
import { banName, endsText, plain } from "../banFormat";
import {
  confirmMoveAll,
  confirmUnban,
  dismissConfirm,
  useAdminConfirm,
  type PendingConfirm,
} from "./confirmStore";

/** Renders the open question of an admin command; mount once next to the chat. */
export function AdminConfirmHost() {
  const confirm = useAdminConfirm();
  if (confirm === null) return null;
  return confirm.kind === "unban" ? (
    <UnbanDialog confirm={confirm} />
  ) : (
    <MoveAllDialog confirm={confirm} />
  );
}

interface QuestionProps {
  title: string;
  serverId: number;
  note: string;
  confirmLabel: string;
  onConfirm: () => void;
}

function Question({ title, serverId, note, confirmLabel, onConfirm }: QuestionProps) {
  const { t: tCommon } = useTranslation("common");
  const { t: tServers } = useTranslation("servers");
  const servers = useServers().servers;
  const server = servers.find((entry) => entry.id === serverId);
  const name = server === undefined ? tServers("unknownServer") : displayName(server);
  const noteId = useId();
  const confirmRef = useRef<HTMLButtonElement>(null);
  return (
    <Dialog
      title={`${title} · ${name}`}
      titleLead={
        <span className="admin-title-mark" aria-hidden="true">
          {initials(name)}
        </span>
      }
      onClose={dismissConfirm}
      describedBy={noteId}
      className="admin-dialog"
      initialFocus={confirmRef}
    >
      {/* A form, because Enter is the answer the spec asks for and must not be a dead key. */}
      <form
        className="admin-form"
        onSubmit={(event) => {
          event.preventDefault();
          onConfirm();
        }}
      >
        <p id={noteId} className="admin-dialog__note">
          {note}
        </p>
        <div className="servers-dialog__actions">
          <button type="button" className="button" onClick={dismissConfirm}>
            {tCommon("cancel")}
          </button>
          <button ref={confirmRef} type="submit" className="button button--primary">
            {confirmLabel}
          </button>
        </div>
      </form>
    </Dialog>
  );
}

function UnbanDialog({ confirm }: { confirm: PendingConfirm & { kind: "unban" } }) {
  const { t, i18n } = useTranslation("admin");
  const entry = confirm.entry;
  const name = banName(entry);
  const ends = endsText(entry, (key) => t(`admin:${key}`), i18n.language);
  return (
    <Question
      title={t("unbanQuestion", { name })}
      serverId={confirm.serverId}
      note={`${t("unbanDetails", {
        reason: plain(entry.reason) || t("unbanReasonMissing"),
        ends: entry.expired || entry.durationSecs === 0 ? ends : t("unbanUntil", { date: ends }),
      })} ${t("unbanNote", { name })}`}
      confirmLabel={t("unban")}
      onConfirm={() => {
        void confirmUnban({
          serverId: confirm.serverId,
          conversation: confirm.conversation,
          state: getServerStateOf(confirm.serverId),
          t,
        });
      }}
    />
  );
}

function MoveAllDialog({ confirm }: { confirm: PendingConfirm & { kind: "moveAll" } }) {
  const { t } = useTranslation("admin");
  return (
    <Question
      title={t("commands.moveAllQuestion", {
        count: confirm.count,
        from: confirm.from,
        to: confirm.to,
      })}
      serverId={confirm.serverId}
      note={t("commands.moveAllNote")}
      confirmLabel={t("commands.moveAll")}
      onConfirm={() => {
        void confirmMoveAll({
          serverId: confirm.serverId,
          conversation: confirm.conversation,
          state: getServerStateOf(confirm.serverId),
          t,
        });
      }}
    />
  );
}
