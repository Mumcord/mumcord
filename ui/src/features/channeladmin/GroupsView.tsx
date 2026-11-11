import { useEffect, useId, useRef, useState, type ReactElement } from "react";
import { useTranslation } from "react-i18next";
import type { AclGroupView, ChannelAclView } from "../../api";
import { SearchableSelect } from "../../components/SearchableSelect";
import { Avatar } from "../members/Avatar";
import { Dialog } from "../servers/Dialog";
import { members, userName } from "./permissionsModel";
import { WhoGrid } from "./WhoGrid";

interface GroupsViewProps {
  serverId: number;
  acl: ChannelAclView;
  channelName: string;
  readOnly: boolean;
  /** Registered users the member picker offers (FR-10.38, FR-10.39). */
  people: { id: number; name: string }[];
  onChange: (acl: ChannelAclView) => void;
  onGoToRule: (index: number) => void;
  /** "Open in Who can do what" from the group's dialog. */
  onOpenWho: () => void;
}

const BUILT_INS = ["all", "auth", "in", "out", "sub", "#token"] as const;
/** The member list gets a filter field above this many members. */
const FILTER_FROM = 6;

const iconProps = {
  width: 16,
  height: 16,
  viewBox: "0 0 20 20",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.8,
  strokeLinecap: "round",
  strokeLinejoin: "round",
  "aria-hidden": true,
  focusable: false,
} as const;

// Decorative: the buttons and rows around them carry the names.
const ICONS: Record<"people" | "plus" | "x" | "undo" | "chevron" | "search", ReactElement> = {
  people: (
    <svg {...iconProps}>
      <circle cx="7.5" cy="7" r="2.8" />
      <path d="M2.5 16c.4-2.8 2.4-4.2 5-4.2s4.6 1.4 5 4.2" />
      <circle cx="14" cy="7.5" r="2.2" />
      <path d="M14.5 11.8c1.8.3 2.8 1.6 3.1 3.6" />
    </svg>
  ),
  plus: (
    <svg {...iconProps}>
      <path d="M10 4v12M4 10h12" />
    </svg>
  ),
  x: (
    <svg {...iconProps}>
      <path d="M5 5l10 10M15 5L5 15" />
    </svg>
  ),
  undo: (
    <svg {...iconProps}>
      <path d="M7 5L3.5 8.5 7 12" />
      <path d="M4 8.5h8a4 4 0 0 1 0 8H8" />
    </svg>
  ),
  chevron: (
    <svg {...iconProps}>
      <path d="M8 5l5 5-5 5" />
    </svg>
  ),
  search: (
    <svg {...iconProps}>
      <circle cx="9" cy="9" r="5" />
      <path d="M13 13l4 4" />
    </svg>
  ),
};

/**
 * The Groups view (FR-10.38): the channel's groups on the left with a prominent "New group" and the
 * built-in ones explained; the chosen group's members on the right ("added here", "inherited",
 * "inherited, removed here"), its options, and a dialog with what the group can do in this channel.
 */
export function GroupsView({
  serverId,
  acl,
  channelName,
  readOnly,
  people,
  onChange,
  onGoToRule,
  onOpenWho,
}: GroupsViewProps) {
  const { t } = useTranslation("admin");
  const nameId = useId();
  const addId = useId();
  const filterId = useId();
  const [selected, setSelected] = useState<string | null>(acl.groups[0]?.name ?? null);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [exists, setExists] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [canDo, setCanDo] = useState(false);
  const newButton = useRef<HTMLButtonElement>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  const wasCreating = useRef(false);
  const group = acl.groups.find((g) => g.name === selected);

  // Focus follows the box: into the name field when it opens, back to "New group" when it closes.
  useEffect(() => {
    if (creating) nameInput.current?.focus();
    else if (wasCreating.current) newButton.current?.focus();
    wasCreating.current = creating;
  }, [creating]);

  const update = (patch: Partial<AclGroupView>) => {
    if (group === undefined) return;
    onChange({
      ...acl,
      groups: acl.groups.map((g) => (g.name === group.name ? { ...g, ...patch } : g)),
    });
  };
  const closeCreate = () => {
    setCreating(false);
    setNewName("");
    setExists(null);
  };
  const create = () => {
    const name = newName.trim();
    if (name === "") return;
    if (acl.groups.some((g) => g.name === name)) {
      setExists(name);
      return;
    }
    onChange({
      ...acl,
      groups: [
        ...acl.groups,
        {
          name,
          inherited: false,
          inherit: true,
          inheritable: true,
          add: [],
          remove: [],
          inheritedMembers: [],
        },
      ],
    });
    setSelected(name);
    setFilter("");
    closeCreate();
  };
  const list = group === undefined ? [] : members(group);
  const active = list.filter((m) => m.state !== "removed").length;
  const added = list.filter((m) => m.state === "added").length;
  const inherited = list.filter((m) => m.state === "inherited").length;
  const needle = filter.trim().toLowerCase();
  const shown =
    list.length < FILTER_FROM || needle === ""
      ? list
      : list.filter((m) => userName(acl, m.id).toLowerCase().includes(needle));
  const candidates = people.filter(
    (p) => !list.some((m) => m.id === p.id && m.state !== "removed"),
  );
  const summary = [
    t("perm.groups.memberCount", { count: active }),
    added > 0 ? t("perm.groups.summaryAdded", { count: added }) : "",
    inherited > 0 ? t("perm.groups.summaryInherited", { count: inherited }) : "",
  ]
    .filter((part) => part !== "")
    .join(" · ");

  return (
    <div className="groups-view">
      <div className="groups-view__list">
        {!readOnly &&
          (creating ? (
            // Escape from any control inside the box cancels it; the box itself is not a control.
            // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions
            <div
              className="groups-view__new"
              role="group"
              aria-label={t("perm.groups.newGroup")}
              onKeyDown={(e) => {
                if (e.key !== "Escape") return;
                // Closes this box only, not the channel window around it.
                e.preventDefault();
                e.stopPropagation();
                closeCreate();
              }}
            >
              <label className="admin-field" htmlFor={nameId}>
                <span>{t("perm.groups.nameLabel")}</span>
                <input
                  id={nameId}
                  ref={nameInput}
                  type="text"
                  value={newName}
                  aria-invalid={exists !== null || undefined}
                  aria-describedby={exists === null ? undefined : `${nameId}-error`}
                  onChange={(e) => {
                    setNewName(e.target.value);
                    setExists(null);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      create();
                    }
                  }}
                />
              </label>
              {exists !== null && (
                <p id={`${nameId}-error`} className="groups-view__error" role="alert">
                  {t("perm.groups.nameExists", { name: exists })}
                </p>
              )}
              <div className="groups-view__new-actions">
                <button type="button" className="button" onClick={closeCreate}>
                  {t("channel.cancel")}
                </button>
                <button
                  type="button"
                  className="button button--primary"
                  disabled={newName.trim() === ""}
                  onClick={create}
                >
                  {t("perm.groups.createGroup")}
                </button>
              </div>
            </div>
          ) : (
            <button
              ref={newButton}
              type="button"
              className="button button--primary groups-view__new-button"
              onClick={() => {
                setCreating(true);
              }}
            >
              {ICONS.plus}
              {t("perm.groups.newGroup")}
            </button>
          ))}
        <h3 className="rules-view__heading">{t("perm.groups.here")}</h3>
        <ul aria-label={t("perm.groups.here")}>
          {acl.groups.map((g) => (
            <li key={g.name}>
              <button
                type="button"
                className="rules-view__rule groups-view__group"
                aria-current={g.name === selected || undefined}
                onClick={() => {
                  setSelected(g.name);
                  setFilter("");
                }}
              >
                <span className="groups-view__group-icon">{ICONS.people}</span>
                <span className="rules-view__who">{g.name}</span>
                <span className="rules-view__badge">
                  {g.inherited
                    ? t("perm.rules.fromParent")
                    : t("perm.groups.memberCount", {
                        count: members(g).filter((m) => m.state !== "removed").length,
                      })}
                </span>
              </button>
            </li>
          ))}
        </ul>
        <h3 className="rules-view__heading">{t("perm.groups.builtIn")}</h3>
        <dl className="groups-view__builtins">
          {BUILT_INS.map((name) => (
            <div key={name}>
              <dt>{name}</dt>
              <dd>{t(`perm.groups.explain.${name === "#token" ? "token" : name}`)}</dd>
            </div>
          ))}
        </dl>
      </div>
      <div className="groups-view__detail">
        {group === undefined ? (
          <p className="channel-window__hint">{t("perm.groups.pick")}</p>
        ) : (
          <>
            <div className="groups-view__head">
              <div className="groups-view__title">
                <h3 className="groups-view__name">{group.name}</h3>
                <p className="groups-view__meta">{summary}</p>
                <button
                  type="button"
                  className="groups-view__link"
                  onClick={() => {
                    setCanDo(true);
                  }}
                >
                  {t("perm.groups.seeCan", { group: group.name, channel: channelName })}
                  {ICONS.chevron}
                </button>
              </div>
              <div className="groups-view__options">
                {(["inherit", "inheritable"] as const).map((key) => (
                  <label key={key} className="admin-switch groups-view__option">
                    <input
                      type="checkbox"
                      role="switch"
                      className="settings-switch__input"
                      checked={group[key]}
                      aria-checked={group[key]}
                      disabled={readOnly}
                      onChange={(e) => {
                        update({ [key]: e.target.checked });
                      }}
                    />
                    <span className="admin-switch__label">{t(`perm.groups.${key}`)}</span>
                    <span className="groups-view__hint">
                      {t(`perm.groups.${key === "inherit" ? "hintInherit" : "hintInheritable"}`)}
                    </span>
                  </label>
                ))}
              </div>
            </div>
            <div className="groups-view__members-head">
              <h3 className="rules-view__heading">{t("perm.groups.members")}</h3>
              <span className="rules-view__badge">{active}</span>
              {list.length >= FILTER_FROM && (
                <span className="groups-view__filter">
                  {ICONS.search}
                  <input
                    id={filterId}
                    type="search"
                    value={filter}
                    placeholder={t("perm.groups.filter")}
                    aria-label={t("perm.groups.filter")}
                    onChange={(e) => {
                      setFilter(e.target.value);
                    }}
                  />
                </span>
              )}
            </div>
            {list.length === 0 ? (
              <div className="groups-view__empty">
                <strong>{t("perm.groups.empty")}</strong>
                <span>{t("perm.groups.emptyHint")}</span>
              </div>
            ) : (
              <ul className="groups-view__members" aria-label={t("perm.groups.members")}>
                {shown.map((m) => {
                  const name = userName(acl, m.id);
                  const label = t(`perm.groups.action.${m.state}`, { name });
                  return (
                    <li key={m.id} data-removed={m.state === "removed" || undefined}>
                      <Avatar name={name} size="sm" />
                      <span className="groups-view__member-name">{name}</span>
                      <span className="groups-view__state" data-state={m.state}>
                        {t(`perm.groups.state.${m.state}`)}
                      </span>
                      {!readOnly &&
                        (m.state === "removed" ? (
                          <button
                            type="button"
                            className="button groups-view__restore"
                            aria-label={label}
                            onClick={() => {
                              update({ remove: group.remove.filter((id) => id !== m.id) });
                            }}
                          >
                            {ICONS.undo}
                            {t("perm.groups.restore")}
                          </button>
                        ) : (
                          <button
                            type="button"
                            className="groups-view__remove"
                            aria-label={label}
                            data-tooltip={label}
                            onClick={() => {
                              if (m.state === "added")
                                update({ add: group.add.filter((id) => id !== m.id) });
                              else update({ remove: [...group.remove, m.id] });
                            }}
                          >
                            {ICONS.x}
                          </button>
                        ))}
                    </li>
                  );
                })}
              </ul>
            )}
            {!readOnly && (
              <div className="admin-field">
                <label htmlFor={addId}>{t("perm.groups.add")}</label>
                <SearchableSelect
                  id={addId}
                  value=""
                  options={candidates.map((p) => ({ value: String(p.id), label: p.name }))}
                  placeholder={t("perm.groups.addSearch")}
                  noMatch={t("perm.groups.noPeople")}
                  onChange={(value) => {
                    const id = Number(value);
                    update({
                      add: [...group.add.filter((a) => a !== id), id],
                      remove: group.remove.filter((r) => r !== id),
                    });
                  }}
                />
                <span className="channel-window__hint">{t("perm.groups.guests")}</span>
              </div>
            )}
            {canDo && (
              <Dialog
                title={t("perm.groups.dialogTitle", { group: group.name, channel: channelName })}
                className="groups-view__dialog"
                onClose={() => {
                  setCanDo(false);
                }}
              >
                <p className="channel-window__hint">{t("perm.groups.dialogHint")}</p>
                <WhoGrid
                  serverId={serverId}
                  acl={acl}
                  channelName={channelName}
                  root={false}
                  checked={null}
                  checkedName=""
                  columns={["enter", "speak", "textMessage", "muteDeafen", "move", "write"]}
                  onlyGroup={group.name}
                  onGoToRule={(index) => {
                    setCanDo(false);
                    onGoToRule(index);
                  }}
                />
                <div className="servers-dialog__actions">
                  <button
                    type="button"
                    className="button"
                    onClick={() => {
                      setCanDo(false);
                      onOpenWho();
                    }}
                  >
                    {t("perm.groups.openWho", { view: t("perm.view.who") })}
                  </button>
                  <button
                    type="button"
                    className="button button--primary"
                    onClick={() => {
                      setCanDo(false);
                    }}
                  >
                    {t("channel.close")}
                  </button>
                </div>
              </Dialog>
            )}
          </>
        )}
      </div>
    </div>
  );
}
