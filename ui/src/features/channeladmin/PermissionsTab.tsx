import { useId, useState, type ReactElement } from "react";
import { useTranslation } from "react-i18next";
import type { AclRowInput, ChannelAclView } from "../../api";
import { SearchableSelect } from "../../components/SearchableSelect";
import { useServerState } from "../channels";
import { errorText } from "../admin/errors";
import { GroupsView } from "./GroupsView";
import { RulesView } from "./RulesView";
import type { AclLoad } from "./useChannelAcl";
import { WhoGrid } from "./WhoGrid";

type View = "who" | "rules" | "groups";

const iconProps = {
  viewBox: "0 0 20 20",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.8,
  strokeLinecap: "round",
  strokeLinejoin: "round",
  "aria-hidden": true,
  focusable: false,
} as const;

// Decorative: each button names itself with its text.
const VIEW_ICONS: Record<View, ReactElement> = {
  who: (
    <svg {...iconProps}>
      <rect x="3" y="3" width="14" height="14" rx="2" />
      <path d="M3 8h14M3 12.5h14M8 3v14" />
    </svg>
  ),
  rules: (
    <svg {...iconProps}>
      <path d="M7 5h10M7 10h10M7 15h10" />
      <circle cx="3.5" cy="5" r=".6" />
      <circle cx="3.5" cy="10" r=".6" />
      <circle cx="3.5" cy="15" r=".6" />
    </svg>
  ),
  groups: (
    <svg {...iconProps}>
      <circle cx="7.5" cy="7" r="2.8" />
      <path d="M2.5 16c.4-2.8 2.4-4.2 5-4.2s4.6 1.4 5 4.2" />
      <circle cx="14" cy="7.5" r="2.2" />
      <path d="M14.5 11.8c1.8.3 2.8 1.6 3.1 3.6" />
    </svg>
  ),
};

const REFRESH_ICON = (
  <svg {...iconProps}>
    <path d="M16 10a6 6 0 1 1-2-4.5" />
    <path d="M16 3.5v3.2h-3.2" />
  </svg>
);

interface PermissionsTabProps {
  serverId: number;
  channelId: number;
  load: AclLoad;
  /** The rules being edited; `null` until they are loaded. */
  draft: ChannelAclView | null;
  readOnly: boolean;
  onChange: (acl: ChannelAclView) => void;
  onRefresh: () => void;
}

/**
 * The Permissions tab (FR-10.36, issue #252, mockups "acl-rules" and "acl-groups"): "Who can do
 * what" (default), "Rules" and "Groups", with "Check a person…" and "Refresh". Every change stays
 * in the window until its one Save.
 */
export function PermissionsTab({
  serverId,
  channelId,
  load,
  draft,
  readOnly,
  onChange,
  onRefresh,
}: PermissionsTabProps) {
  const { t } = useTranslation("admin");
  const server = useServerState();
  const checkId = useId();
  const [view, setView] = useState<View>("who");
  const [rule, setRule] = useState<number | null>(null);
  const [checkedPick, setChecked] = useState<{ row: AclRowInput; name: string } | null>(null);
  // The checked person may disconnect; their session is gone, so there is nothing left to check.
  const checked =
    checkedPick?.row.kind === "session" && !server.users.has(checkedPick.row.session)
      ? null
      : checkedPick;

  if (load.status === "idle" || load.status === "loading" || draft === null) {
    if (load.status === "failed") {
      return (
        <div className="channel-window__failed" role="alert">
          <p>{errorText(t, load.error)}</p>
          <button type="button" className="button" onClick={onRefresh}>
            {t("channel.retry")}
          </button>
        </div>
      );
    }
    return <p aria-busy="true">{t("channel.rulesLoading")}</p>;
  }

  const channel = server.channels.get(channelId);
  const parent = channel?.parentId == null ? undefined : server.channels.get(channel.parentId);
  const root = channelId === 0;
  // Registered people Mumcord knows: everyone online who is registered, and those named in the
  // rules (FR-10.38); the engine collects both when it reads the rules.
  const registered = draft.userNames.map((u) => ({ id: u.id, name: u.name }));
  const online = [...server.users.values()].map((u) => ({
    value: String(u.session),
    label: u.name,
  }));
  const counts = { rules: draft.rules.length, groups: draft.groups.length };
  const goToRule = (index: number) => {
    setRule(index);
    setView("rules");
  };

  return (
    <div className="permissions-tab">
      {readOnly && (
        <p className="channel-window__note" role="status">
          {t("perm.readOnly")}
        </p>
      )}
      <div className="permissions-tab__bar">
        <div className="permissions-tab__segments" role="group" aria-label={t("perm.views")}>
          {(["who", "rules", "groups"] as const).map((name) => {
            const count = name === "who" ? null : counts[name];
            const label = t(`perm.view.${name}`);
            return (
              <button
                key={name}
                type="button"
                className="permissions-tab__segment"
                aria-pressed={view === name}
                aria-label={
                  count === null ? undefined : t("perm.viewCount", { view: label, count })
                }
                onClick={() => {
                  setView(name);
                }}
              >
                {VIEW_ICONS[name]}
                {label}
                {count !== null && (
                  <span className="permissions-tab__count" aria-hidden="true">
                    {count}
                  </span>
                )}
              </button>
            );
          })}
        </div>
        {view === "who" && (
          <div className="permissions-tab__check">
            <label htmlFor={checkId}>{t("perm.check")}</label>
            <SearchableSelect
              id={checkId}
              value={checked?.row.kind === "session" ? String(checked.row.session) : ""}
              options={online}
              placeholder={t("perm.checkSearch")}
              noMatch={t("perm.groups.noPeople")}
              clearLabel={t("perm.checkClear", { name: checked?.name ?? "" })}
              onChange={(value) => {
                if (value === "") {
                  setChecked(null);
                  return;
                }
                const session = Number(value);
                const user = server.users.get(session);
                setChecked(
                  user === undefined
                    ? null
                    : { row: { kind: "session", session }, name: user.name },
                );
              }}
            />
          </div>
        )}
        <button
          type="button"
          className="permissions-tab__refresh"
          aria-label={t("perm.refresh")}
          data-tooltip={t("perm.refresh")}
          onClick={onRefresh}
        >
          {REFRESH_ICON}
        </button>
      </div>
      {view === "who" && (
        <WhoGrid
          serverId={serverId}
          acl={draft}
          channelName={channel?.name ?? ""}
          root={root}
          checked={checked?.row ?? null}
          checkedName={checked?.name ?? ""}
          onGoToRule={goToRule}
        />
      )}
      {view === "rules" && (
        <RulesView
          serverId={serverId}
          acl={draft}
          root={root}
          parentName={parent?.name ?? ""}
          readOnly={readOnly}
          people={registered}
          selected={rule}
          onSelect={setRule}
          onChange={onChange}
        />
      )}
      {view === "groups" && (
        <GroupsView
          serverId={serverId}
          acl={draft}
          channelName={channel?.name ?? ""}
          readOnly={readOnly}
          people={registered}
          onChange={onChange}
          onGoToRule={goToRule}
          onOpenWho={() => {
            setView("who");
          }}
        />
      )}
    </div>
  );
}
