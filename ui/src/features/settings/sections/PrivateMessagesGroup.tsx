import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { PmRule, PrivateMessageSettings } from "../../../api";
import { Select } from "../../../components/Select";
import { displayName } from "../../servers/format";
import { useServers } from "../../servers/store";
import { Group } from "../components/Section";
import { ChangedBadge, RecommendedBadge } from "../components/SettingBadges";
import { Switch } from "../components/Switch";
import type { SettingsSectionProps } from "./types";
import "./privateMessages.css";

const PM_RULES: readonly PmRule[] = ["everyone", "friendsOnly", "nobody"];

const USE_DEFAULT = "default";

/** "Private messages from" with per-server overrides and exceptions (issue #141). */
export function PrivateMessagesGroup({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const pm = settings.privateMessages;
  const change = (patch: Partial<PrivateMessageSettings>) => {
    update({ privateMessages: { ...pm, ...patch } });
  };
  return (
    <>
      <Group title={t("privateMessages.title")} icon="mail">
        <PmRuleCards
          value={pm.rule}
          onChange={(rule) => {
            change({ rule });
          }}
        />
        <Switch
          label={t("privateMessages.keepRequests")}
          description={t("privateMessages.keepRequestsHint")}
          checked={pm.keepRequests}
          onChange={(keepRequests) => {
            change({ keepRequests });
          }}
        />
      </Group>
      <ServerOverrides
        settings={pm}
        onChange={(serverRules) => {
          change({ serverRules });
        }}
      />
      <Group title={t("privateMessages.alwaysTitle")} icon="user">
        <Switch
          label={t("privateMessages.currentChannel")}
          description={t("privateMessages.currentChannelHint")}
          checked={pm.allowCurrentChannel}
          onChange={(allowCurrentChannel) => {
            change({ allowCurrentChannel });
          }}
        />
        <Switch
          label={t("privateMessages.accepted")}
          description={t("privateMessages.acceptedHint")}
          checked={pm.allowAccepted}
          onChange={(allowAccepted) => {
            change({ allowAccepted });
          }}
        />
      </Group>
    </>
  );
}

/** The rule #159 recommends; its card carries the "Recommended" pill. */
const RECOMMENDED_RULE: PmRule = "friendsOnly";

/**
 * Native radios drawn as cards; the title names each radio, the hint describes it. The group is
 * named by the legend and its "Changed" badge, which stays outside the legend.
 */
export function PmRuleCards({
  value,
  onChange,
}: {
  value: PmRule;
  onChange: (rule: PmRule) => void;
}) {
  const { t } = useTranslation("settings");
  const id = useId();
  return (
    <fieldset className="pm-rules" aria-labelledby={`${id}-legend ${id}-badges`}>
      <legend id={`${id}-legend`} className="pm-rules__legend">
        {t("privateMessages.legend")}
      </legend>
      <span id={`${id}-badges`} className="setting-badges">
        <ChangedBadge label={t("privateMessages.legend")} />
      </span>
      {PM_RULES.map((rule) => {
        const titleId = `${id}-${rule}-title`;
        const hintId = `${id}-${rule}-hint`;
        return (
          <label key={rule} className="pm-rules__card">
            <input
              type="radio"
              name={id}
              value={rule}
              checked={rule === value}
              aria-labelledby={titleId}
              aria-describedby={hintId}
              data-testid={`pm-rule-${rule}`}
              onChange={() => {
                onChange(rule);
              }}
            />
            <span className="pm-rules__text">
              <span id={titleId} className="pm-rules__title">
                {t(`privateMessages.${rule}`)}
                {rule === RECOMMENDED_RULE && (
                  <>
                    {" "}
                    <RecommendedBadge />
                  </>
                )}
              </span>
              <span id={hintId} className="pm-rules__hint">
                {t(`privateMessages.${rule}Hint`)}
              </span>
            </span>
          </label>
        );
      })}
    </fieldset>
  );
}

function ServerOverrides({
  settings,
  onChange,
}: {
  settings: PrivateMessageSettings;
  onChange: (serverRules: PrivateMessageSettings["serverRules"]) => void;
}) {
  const { t } = useTranslation("settings");
  const { servers } = useServers();
  const options = [
    {
      value: USE_DEFAULT,
      label: t("privateMessages.useDefault", { rule: t(`privateMessages.${settings.rule}`) }),
    },
    ...PM_RULES.map((rule) => ({ value: rule, label: t(`privateMessages.${rule}`) })),
  ];
  const choose = (serverId: number, value: string) => {
    const others = settings.serverRules.filter((r) => r.serverId !== serverId);
    const rule = PM_RULES.find((r) => r === value);
    onChange(rule === undefined ? others : [...others, { serverId, rule }]);
  };
  return (
    <Group title={t("privateMessages.overrideTitle")} icon="server">
      {servers.length === 0 ? (
        <p className="settings-hint">{t("privateMessages.noServers")}</p>
      ) : (
        <div className="pm-overrides">
          {servers.map((server) => {
            const name = displayName(server);
            const current = settings.serverRules.find((r) => r.serverId === server.id);
            return (
              <div key={server.id} className="settings-control-row pm-overrides__row">
                <span className="pm-overrides__server">{name}</span>
                <Select
                  ariaLabel={t("privateMessages.overrideLabel", { server: name })}
                  value={current?.rule ?? USE_DEFAULT}
                  options={options}
                  onChange={(value) => {
                    choose(server.id, value);
                  }}
                />
              </div>
            );
          })}
        </div>
      )}
    </Group>
  );
}
