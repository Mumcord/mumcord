import { useId, useState } from "react";
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
  const [checked, setChecked] = useState<{ row: AclRowInput; name: string } | null>(null);

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
        <div className="admin-chips" role="group" aria-label={t("perm.views")}>
          {(["who", "rules", "groups"] as const).map((name) => (
            <button
              key={name}
              type="button"
              className="button button--small"
              aria-pressed={view === name}
              onClick={() => {
                setView(name);
              }}
            >
              {t(`perm.view.${name}`)}
            </button>
          ))}
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
              onChange={(value) => {
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
        <button type="button" className="button button--small" onClick={onRefresh}>
          {t("perm.refresh")}
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
        />
      )}
    </div>
  );
}
