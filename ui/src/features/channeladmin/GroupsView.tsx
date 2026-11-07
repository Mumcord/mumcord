import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import type { AclGroupView, ChannelAclView } from "../../api";
import { SearchableSelect } from "../../components/SearchableSelect";
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
}

const BUILT_INS = ["all", "auth", "in", "out", "sub", "#token"] as const;

/**
 * The Groups view (FR-10.38, mockup "acl-groups"): the channel's groups on the left with the
 * built-in ones explained; the chosen group's members on the right ("added here", "inherited",
 * "inherited, removed here"), its options, and a short "who can do what" for this channel.
 */
export function GroupsView({
  serverId,
  acl,
  channelName,
  readOnly,
  people,
  onChange,
  onGoToRule,
}: GroupsViewProps) {
  const { t } = useTranslation("admin");
  const nameId = useId();
  const addId = useId();
  const [selected, setSelected] = useState<string | null>(acl.groups[0]?.name ?? null);
  const [newName, setNewName] = useState("");
  const group = acl.groups.find((g) => g.name === selected);

  const update = (patch: Partial<AclGroupView>) => {
    if (group === undefined) return;
    onChange({
      ...acl,
      groups: acl.groups.map((g) => (g.name === group.name ? { ...g, ...patch } : g)),
    });
  };
  const create = () => {
    const name = newName.trim();
    if (name === "" || acl.groups.some((g) => g.name === name)) return;
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
    setNewName("");
  };
  const list = group === undefined ? [] : members(group);
  const candidates = people.filter(
    (p) => !list.some((m) => m.id === p.id && m.state !== "removed"),
  );

  return (
    <div className="groups-view">
      <div className="groups-view__list">
        <h3 className="rules-view__heading">{t("perm.groups.here")}</h3>
        <ul aria-label={t("perm.groups.here")}>
          {acl.groups.map((g) => (
            <li key={g.name}>
              <button
                type="button"
                className="rules-view__rule"
                aria-current={g.name === selected || undefined}
                onClick={() => {
                  setSelected(g.name);
                }}
              >
                <span className="rules-view__who">{g.name}</span>
                <span className="rules-view__badge">
                  {g.inherited
                    ? t("perm.rules.fromParent")
                    : String(members(g).filter((m) => m.state !== "removed").length)}
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
        {!readOnly && (
          <div className="groups-view__new">
            <label className="admin-field">
              <span>{t("perm.groups.newName")}</span>
              <input
                type="text"
                value={newName}
                onChange={(e) => {
                  setNewName(e.target.value);
                }}
              />
            </label>
            <button
              type="button"
              className="button"
              disabled={newName.trim() === ""}
              onClick={create}
            >
              {t("perm.groups.create")}
            </button>
          </div>
        )}
      </div>
      <div className="groups-view__detail">
        {group === undefined ? (
          <p className="channel-window__hint">{t("perm.groups.pick")}</p>
        ) : (
          <>
            <div className="rules-view__head">
              <div className="admin-field">
                <label htmlFor={nameId}>{t("channel.name")}</label>
                <input id={nameId} type="text" value={group.name} readOnly />
              </div>
              <div className="groups-view__options">
                {(["inherit", "inheritable"] as const).map((key) => (
                  <label key={key} className="admin-switch">
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
                  </label>
                ))}
              </div>
            </div>
            <h3 className="rules-view__heading">{t("perm.groups.members")}</h3>
            <ul className="groups-view__members" aria-label={t("perm.groups.members")}>
              {list.map((m) => (
                <li key={m.id} data-removed={m.state === "removed" || undefined}>
                  <span>{userName(acl, m.id)}</span>
                  <span className="rules-view__badge">{t(`perm.groups.state.${m.state}`)}</span>
                  {!readOnly && (
                    <button
                      type="button"
                      className="button button--small"
                      onClick={() => {
                        if (m.state === "added")
                          update({ add: group.add.filter((id) => id !== m.id) });
                        else if (m.state === "inherited")
                          update({ remove: [...group.remove, m.id] });
                        else update({ remove: group.remove.filter((id) => id !== m.id) });
                      }}
                    >
                      {t(`perm.groups.action.${m.state}`, { name: userName(acl, m.id) })}
                    </button>
                  )}
                </li>
              ))}
            </ul>
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
            <h3 className="rules-view__heading">
              {t("perm.groups.summary", { channel: channelName })}
            </h3>
            <WhoGrid
              serverId={serverId}
              acl={acl}
              channelName={channelName}
              root={false}
              checked={null}
              checkedName=""
              columns={["enter", "speak", "move", "write"]}
              onGoToRule={onGoToRule}
            />
          </>
        )}
      </div>
    </div>
  );
}
