import { useEffect, useId, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type ChannelResult } from "../../api";
import { SearchableSelect } from "../../components/SearchableSelect";
import { useServerState } from "../channels";
import { channelPath } from "../channelheader/channelModel";
import { sanitizeHtml } from "../chat/sanitize";
import { Dialog } from "../servers/Dialog";
import { displayName } from "../servers/format";
import { useServers } from "../servers/store";
import { RichCommentEditor } from "../userpanel/RichCommentEditor";
import { commentToSend } from "../userpanel/commentHtml";
import {
  changeOf,
  descendants,
  draftOf,
  hasChanges,
  parentOptions,
  refusalText,
  type ChannelDraft,
} from "./channelAdminModel";
import "../admin/admin.css";
import "../settings/settings.css";
import "../userpanel/userpanel.css";
import "./channeladmin.css";

export type ChannelWindowMode =
  { kind: "create"; parentId: number; temporary: boolean } | { kind: "edit"; channelId: number };

type Tab = "general" | "links";

interface ChannelWindowProps {
  mode: ChannelWindowMode;
  onClose: () => void;
  /** Opens the delete confirmation; edit mode only, and only with the right. */
  onDelete?: (() => void) | undefined;
}

/**
 * The Channel window (Epic 4, issue #250, mockup "Edit channel"): create a sub-channel or a
 * temporary channel, or edit, move and link a channel. Create mode shows only the General tab
 * (FR-10.21); one Save writes every changed field, and only those (FR-10.23). Moving under another
 * parent asks first (FR-10.24). Each change waits for the server, and a refusal shows its reason
 * here (FR-10.59).
 */
export function ChannelWindow({ mode, onClose, onDelete }: ChannelWindowProps) {
  const { t } = useTranslation("admin");
  const server = useServerState();
  const listed = useServers().servers.find((s) => s.id === server.serverId);
  const serverName = listed ? displayName(listed) : "";
  const ids = { name: useId(), parent: useId(), description: useId(), hint: useId() };
  const ids2 = { maxUsers: useId(), position: useId(), temporary: useId() };
  const editing = mode.kind === "edit";
  const channel = editing ? server.channels.get(mode.channelId) : undefined;
  const original = useMemo(
    () => {
      const draft = draftOf(
        channel ?? null,
        mode.kind === "create" ? mode.parentId : 0,
        mode.kind === "create" && mode.temporary,
      );
      // Compared in the cleaned form the editor sends, so opening and saving changes nothing.
      return { ...draft, description: commentToSend(draft.description) };
    },
    // The draft starts from the channel as it was when the window opened.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );
  const [draft, setDraft] = useState<ChannelDraft>(original);
  const [tab, setTab] = useState<Tab>("general");
  const [preview, setPreview] = useState(false);
  const [search, setSearch] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmMove, setConfirmMove] = useState(false);

  const serverId = server.serverId;
  // The root channel has no path of its own; it shows under its name.
  const path = (id: number) =>
    channelPath(server.channels, id).join(" › ") || (server.channels.get(id)?.name ?? "");
  const update = (patch: Partial<ChannelDraft>) => {
    setDraft((current) => ({ ...current, ...patch }));
    setError(null);
  };

  const change = changeOf(original, { ...draft, description: commentToSend(draft.description) });
  const dirty = editing ? hasChanges(change) : true;
  const nameMissing = draft.name.trim() === "";

  const finish = (result: ChannelResult) => {
    const refusal = refusalText(t, result);
    if (refusal !== null) {
      setError(refusal);
      return;
    }
    onClose();
  };

  const send = async () => {
    if (serverId === null || nameMissing) return;
    setBusy(true);
    setError(null);
    try {
      if (mode.kind === "create") {
        const description = commentToSend(draft.description);
        finish(
          await getApi().channelCreate(serverId, {
            parentId: draft.parentId,
            name: draft.name.trim(),
            description: description === "" ? null : description,
            position: draft.position,
            maxUsers: draft.maxUsers,
            temporary: draft.temporary,
          }),
        );
      } else {
        finish(await getApi().channelEdit(serverId, mode.channelId, change));
      }
    } catch (failure) {
      console.error("Saving the channel failed", failure);
      setError(t("channel.failed"));
    } finally {
      setBusy(false);
    }
  };

  const save = () => {
    if (busy || nameMissing || !dirty) return;
    if (editing && change.parentId !== null) {
      setConfirmMove(true);
      return;
    }
    void send();
  };

  // Ctrl+Enter saves from any field, as the mockup's keyboard note says.
  const saveRef = useRef(save);
  useEffect(() => {
    saveRef.current = save;
  });
  useEffect(() => {
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
        event.preventDefault();
        saveRef.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  if (confirmMove && mode.kind === "edit") {
    const moving = descendants(server.channels, mode.channelId).size;
    return (
      <Dialog
        role="alertdialog"
        className="channel-confirm"
        title={t("channel.moveTitle", {
          channel: draft.name.trim(),
          parent: server.channels.get(draft.parentId)?.name ?? "",
        })}
        onClose={() => {
          setConfirmMove(false);
        }}
      >
        <p>{moving === 0 ? t("channel.moveAlone") : t("channel.moveWith", { count: moving })}</p>
        <div className="servers-dialog__actions">
          <button
            type="button"
            className="button"
            onClick={() => {
              setConfirmMove(false);
            }}
          >
            {t("channel.cancel")}
          </button>
          <button
            type="button"
            className="button button--primary"
            onClick={() => {
              setConfirmMove(false);
              void send();
            }}
          >
            {t("channel.moveConfirm")}
          </button>
        </div>
      </Dialog>
    );
  }

  const title =
    mode.kind === "edit"
      ? t("channel.editTitle")
      : mode.temporary
        ? t("channel.createTempTitle")
        : t("channel.createTitle");
  const where =
    mode.kind === "edit" ? [serverName, path(mode.channelId)] : [serverName, path(mode.parentId)];
  const parents = parentOptions(server.channels, editing ? mode.channelId : null).map((c) => ({
    value: String(c.id),
    label: path(c.id),
    keywords: [c.name],
  }));
  const linkable =
    mode.kind === "edit"
      ? [...server.channels.values()]
          .filter((c) => c.id !== mode.channelId)
          .filter((c) => c.name.toLowerCase().includes(search.trim().toLowerCase()))
      : [];

  return (
    <Dialog
      className="channel-window"
      title={title}
      onClose={onClose}
      closeLabel={t("channel.close")}
    >
      {/* The path names the server and where the channel sits, like the mockup's subtitle. */}
      <p className="channel-window__path">{where.filter((part) => part !== "").join(" › ")}</p>
      {editing && (
        <div role="tablist" aria-label={title} className="channel-window__tabs">
          {(["general", "links"] as const).map((name) => (
            <button
              key={name}
              type="button"
              role="tab"
              id={`channel-tab-${name}`}
              aria-selected={tab === name}
              aria-controls={`channel-panel-${name}`}
              className="channel-window__tab"
              onClick={() => {
                setTab(name);
              }}
            >
              {name === "general" ? t("channel.tabGeneral") : t("channel.tabLinks")}
            </button>
          ))}
        </div>
      )}
      <div
        className="channel-window__body"
        {...(editing
          ? {
              role: "tabpanel",
              id: `channel-panel-${tab}`,
              "aria-labelledby": `channel-tab-${tab}`,
            }
          : {})}
      >
        {tab === "general" ? (
          <>
            <div className="channel-window__row">
              <div className="channel-window__field admin-field">
                <label htmlFor={ids.name}>{t("channel.name")}</label>
                <input
                  id={ids.name}
                  type="text"
                  value={draft.name}
                  aria-invalid={nameMissing}
                  onChange={(e) => {
                    update({ name: e.target.value });
                  }}
                />
              </div>
              {editing && (
                <div className="channel-window__field admin-field">
                  <label htmlFor={ids.parent}>{t("channel.parent")}</label>
                  <SearchableSelect
                    id={ids.parent}
                    value={String(draft.parentId)}
                    options={parents}
                    placeholder={t("channel.parentSearch")}
                    noMatch={t("channel.noMatch")}
                    onChange={(value) => {
                      update({ parentId: Number(value) });
                    }}
                  />
                </div>
              )}
            </div>
            <div className="channel-window__field admin-field">
              <div className="channel-window__label-row">
                <span id={ids.description}>{t("channel.description")}</span>
                <button
                  type="button"
                  className="button button--link"
                  aria-pressed={preview}
                  onClick={() => {
                    setPreview((on) => !on);
                  }}
                >
                  {t("channel.preview")}
                </button>
              </div>
              <div hidden={preview}>
                <RichCommentEditor
                  labelId={ids.description}
                  describedBy={ids.hint}
                  initialHtml={original.description}
                  loadKey={0}
                  disabled={busy}
                  tools="description"
                  onChange={(html) => {
                    update({ description: html });
                  }}
                />
              </div>
              {preview && (
                <div
                  className="channel-window__preview"
                  aria-labelledby={ids.description}
                  // Sanitized exactly like chat messages and the channel info (FR-10.26).
                  dangerouslySetInnerHTML={{ __html: sanitizeHtml(draft.description) }}
                />
              )}
              <p id={ids.hint} className="channel-window__hint">
                {t("channel.descriptionHint")}
              </p>
            </div>
            <div className="channel-window__row">
              <div className="channel-window__field admin-field">
                <label htmlFor={ids2.maxUsers}>{t("channel.maxUsers")}</label>
                <div className="channel-window__number">
                  <input
                    id={ids2.maxUsers}
                    type="number"
                    min={0}
                    value={draft.maxUsers}
                    aria-describedby={`${ids2.maxUsers}-hint`}
                    onChange={(e) => {
                      update({ maxUsers: Math.max(0, Math.trunc(Number(e.target.value) || 0)) });
                    }}
                  />
                  <span id={`${ids2.maxUsers}-hint`}>{t("channel.maxUsersHint")}</span>
                </div>
              </div>
              <div className="channel-window__field admin-field">
                <label htmlFor={ids2.position}>{t("channel.position")}</label>
                <div className="channel-window__number">
                  <input
                    id={ids2.position}
                    type="number"
                    value={draft.position}
                    aria-describedby={`${ids2.position}-hint`}
                    onChange={(e) => {
                      update({ position: Math.trunc(Number(e.target.value) || 0) });
                    }}
                  />
                  <span id={`${ids2.position}-hint`}>{t("channel.positionHint")}</span>
                </div>
              </div>
            </div>
            <div className="admin-switch">
              <input
                id={ids2.temporary}
                type="checkbox"
                role="switch"
                className="settings-switch__input"
                checked={draft.temporary}
                aria-checked={draft.temporary}
                disabled={editing}
                aria-describedby={`${ids2.temporary}-hint`}
                onChange={(e) => {
                  update({ temporary: e.target.checked });
                }}
              />
              <span className="admin-switch__text">
                <label htmlFor={ids2.temporary} className="admin-switch__label">
                  {t("channel.temporary")}
                </label>
                <span id={`${ids2.temporary}-hint`} className="admin-form__help">
                  {t("channel.temporaryHint")}
                </span>
              </span>
            </div>
            {editing && <p className="channel-window__note">{t("channel.linksNote")}</p>}
          </>
        ) : (
          <>
            <label className="admin-field">
              <span>{t("channel.linksSearch")}</span>
              <input
                type="search"
                value={search}
                onChange={(e) => {
                  setSearch(e.target.value);
                }}
              />
            </label>
            <fieldset className="channel-window__links">
              <legend>{t("channel.linksLabel")}</legend>
              {linkable.length === 0 && <p>{t("channel.noMatch")}</p>}
              {linkable.map((c) => {
                const allowed = c.permissions.linkChannel;
                const reason = `channel-link-${String(c.id)}-reason`;
                return (
                  <label
                    key={c.id}
                    className="channel-window__link"
                    data-disabled={!allowed || undefined}
                  >
                    <input
                      type="checkbox"
                      checked={draft.links.includes(c.id)}
                      disabled={!allowed}
                      aria-describedby={allowed ? undefined : reason}
                      onChange={(e) => {
                        update({
                          links: e.target.checked
                            ? [...draft.links, c.id]
                            : draft.links.filter((id) => id !== c.id),
                        });
                      }}
                    />
                    {path(c.id)}
                    {!allowed && (
                      <span id={reason} className="channel-window__hint">
                        {t("channel.linkNotAllowed")}
                      </span>
                    )}
                  </label>
                );
              })}
            </fieldset>
          </>
        )}
      </div>
      {error !== null && (
        <p role="alert" className="channel-window__error">
          {error}
        </p>
      )}
      <div className="channel-window__footer">
        {onDelete && (
          <button type="button" className="button servers-button--danger" onClick={onDelete}>
            {t("channel.delete")}
          </button>
        )}
        <span className="channel-window__status" aria-live="polite">
          {editing && dirty && !busy ? t("channel.unsaved") : ""}
        </span>
        <button type="button" className="button" onClick={onClose}>
          {t("channel.cancel")}
        </button>
        <button
          type="button"
          className="button button--primary"
          disabled={busy || nameMissing || !dirty}
          onClick={save}
        >
          {busy ? t("channel.saving") : editing ? t("channel.save") : t("channel.create")}
        </button>
      </div>
    </Dialog>
  );
}
