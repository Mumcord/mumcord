import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  getApi,
  type AclCell,
  type AclRowInput,
  type AclRuleView,
  type ChannelAclView,
} from "../../api";
import { SearchableSelect } from "../../components/SearchableSelect";
import { newRule, PERM, permissionsFor, ruleKind, ruleWho } from "./permissionsModel";

interface RulesViewProps {
  serverId: number;
  acl: ChannelAclView;
  root: boolean;
  parentName: string;
  readOnly: boolean;
  /** People the "Who" picker offers besides groups: registered users, by id. */
  people: { id: number; name: string }[];
  selected: number | null;
  onSelect: (index: number | null) => void;
  onChange: (acl: ChannelAclView) => void;
}

/**
 * The Rules view (FR-10.37, mockup "acl-rules"): inherited rules first, grey and read-only, then the
 * channel's own ones; later rules win. The selected own rule is edited on the right: who, where it
 * applies, and Allow or Deny per permission, with the result here worked out by the core.
 */
export function RulesView({
  serverId,
  acl,
  root,
  parentName,
  readOnly,
  people,
  selected,
  onSelect,
  onChange,
}: RulesViewProps) {
  const { t } = useTranslation("admin");
  const inheritId = useId();
  const whoId = useId();
  const rule = selected === null ? undefined : acl.rules[selected];
  const editable = rule !== undefined && !rule.inherited && !readOnly;

  const setRules = (rules: AclRuleView[], select: number | null) => {
    onChange({ ...acl, rules });
    onSelect(select);
  };
  const update = (patch: Partial<AclRuleView>) => {
    if (selected === null || rule === undefined) return;
    setRules(
      acl.rules.map((r, i) => (i === selected ? { ...r, ...patch } : r)),
      selected,
    );
  };
  const move = (by: -1 | 1) => {
    if (selected === null) return;
    const target = selected + by;
    const other = acl.rules[target];
    if (other === undefined || other.inherited || rule === undefined) return;
    const rules = [...acl.rules];
    rules[target] = rule;
    rules[selected] = other;
    setRules(rules, target);
  };

  const groupNames = ["all", "auth", "in", "out", ...acl.groups.map((g) => g.name)].filter(
    (name, i, all) => all.indexOf(name) === i,
  );
  const whoOptions = [
    ...groupNames.map((name) => ({
      value: `g:${name}`,
      label: t("perm.rules.whoGroup", { name }),
    })),
    ...people.map((p) => ({
      value: `u:${String(p.id)}`,
      label: t("perm.rules.whoPerson", { name: p.name }),
    })),
  ];
  const whoValue =
    rule === undefined
      ? ""
      : rule.userId !== null
        ? `u:${String(rule.userId)}`
        : `g:${rule.group ?? ""}`;

  const toggle = (bit: number, side: "grant" | "deny", on: boolean) => {
    if (rule === undefined) return;
    const other = side === "grant" ? "deny" : "grant";
    update({
      [side]: on ? rule[side] | bit : rule[side] & ~bit,
      // Allow and Deny of one permission exclude each other.
      [other]: on ? rule[other] & ~bit : rule[other],
    });
  };

  const own = acl.rules.filter((r) => !r.inherited).length;
  const firstOwn = acl.rules.findIndex((r) => !r.inherited);

  return (
    <div className="rules-view">
      <div className="rules-view__list">
        <div className="admin-switch">
          <input
            id={inheritId}
            type="checkbox"
            role="switch"
            className="settings-switch__input"
            checked={acl.inheritAcls}
            aria-checked={acl.inheritAcls}
            disabled={readOnly || root}
            onChange={(e) => {
              onChange({ ...acl, inheritAcls: e.target.checked });
            }}
          />
          <span className="admin-switch__text">
            <label htmlFor={inheritId} className="admin-switch__label">
              {t("perm.rules.inherit", { parent: parentName })}
            </label>
            <span className="admin-form__help">{t("perm.rules.inheritHint")}</span>
          </span>
        </div>
        <h3 className="rules-view__heading">{t("perm.rules.order")}</h3>
        <ol className="rules-view__rules" aria-label={t("perm.rules.order")}>
          {acl.rules.map((r, i) => {
            const off = r.inherited && !acl.inheritAcls;
            return (
              <li key={i} hidden={off}>
                <button
                  type="button"
                  className="rules-view__rule"
                  aria-current={selected === i || undefined}
                  data-inherited={r.inherited || undefined}
                  onClick={() => {
                    onSelect(i);
                  }}
                >
                  <span className="rules-view__n">{i + 1}</span>
                  <span className="rules-view__who">{ruleWho(acl, r)}</span>
                  <span className="rules-view__badge">
                    {r.inherited ? t("perm.rules.fromParent") : t(`perm.rules.kind.${ruleKind(r)}`)}
                  </span>
                </button>
              </li>
            );
          })}
        </ol>
        {!readOnly && (
          <div className="rules-view__actions">
            <button
              type="button"
              className="button"
              onClick={() => {
                setRules([...acl.rules, newRule()], acl.rules.length);
              }}
            >
              {t("perm.rules.add")}
            </button>
            <button
              type="button"
              className="button"
              disabled={!editable || selected === firstOwn}
              aria-label={t("perm.rules.up")}
              onClick={() => {
                move(-1);
              }}
            >
              ↑
            </button>
            <button
              type="button"
              className="button"
              disabled={!editable || selected === acl.rules.length - 1}
              aria-label={t("perm.rules.down")}
              onClick={() => {
                move(1);
              }}
            >
              ↓
            </button>
            <button
              type="button"
              className="button"
              disabled={!editable}
              onClick={() => {
                if (selected === null) return;
                setRules(
                  acl.rules.filter((_, i) => i !== selected),
                  null,
                );
              }}
            >
              {t("perm.rules.remove")}
            </button>
          </div>
        )}
        <p className="channel-window__note">{t("perm.rules.note", { count: own })}</p>
      </div>
      <div className="rules-view__editor">
        {rule === undefined ? (
          <p className="channel-window__hint">{t("perm.rules.pick")}</p>
        ) : (
          <>
            <div className="rules-view__head">
              <div className="admin-field">
                <label htmlFor={whoId}>{t("perm.rules.appliesTo")}</label>
                {editable ? (
                  <SearchableSelect
                    id={whoId}
                    value={whoValue}
                    options={whoOptions}
                    placeholder={t("perm.rules.whoSearch")}
                    noMatch={t("channel.noMatch")}
                    onChange={(value) => {
                      if (value.startsWith("u:"))
                        update({ userId: Number(value.slice(2)), group: null });
                      else update({ userId: null, group: value.slice(2) });
                    }}
                  />
                ) : (
                  <input id={whoId} type="text" value={ruleWho(acl, rule)} readOnly />
                )}
              </div>
              <div
                className="rules-view__scope admin-chips"
                role="group"
                aria-label={t("perm.rules.scope")}
              >
                <span>{t("perm.rules.scope")}</span>
                {(["applyHere", "applySubs"] as const).map((key) => (
                  <button
                    key={key}
                    type="button"
                    className="button button--small"
                    aria-pressed={rule[key]}
                    disabled={!editable}
                    onClick={() => {
                      update({ [key]: !rule[key] });
                    }}
                  >
                    {t(`perm.rules.${key}`)}
                  </button>
                ))}
              </div>
            </div>
            <RuleTable
              serverId={serverId}
              acl={acl}
              rule={rule}
              root={root}
              editable={editable}
              onToggle={toggle}
            />
          </>
        )}
      </div>
    </div>
  );
}

/** Permission | Result here | Allow | Deny, for the selected rule (mockup "acl-rules"). */
function RuleTable({
  serverId,
  acl,
  rule,
  root,
  editable,
  onToggle,
}: {
  serverId: number;
  acl: ChannelAclView;
  rule: AclRuleView;
  root: boolean;
  editable: boolean;
  onToggle: (bit: number, side: "grant" | "deny", on: boolean) => void;
}) {
  const { t } = useTranslation("admin");
  const [result, setResult] = useState<AclCell[] | null>(null);
  const row: AclRowInput =
    rule.userId !== null
      ? { kind: "user", userId: rule.userId }
      : rule.group === "all"
        ? { kind: "everyone" }
        : { kind: "group", name: rule.group ?? "" };

  useEffect(() => {
    let live = true;
    getApi()
      .channelAclEvaluate(serverId, acl, [row])
      .then((rows) => {
        if (live) setResult(rows[0] ?? null);
      })
      .catch((error: unknown) => {
        console.error("Working out the permissions failed", error);
      });
    return () => {
      live = false;
    };
    // `row` follows from `rule`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverId, acl, rule]);

  return (
    <table className="rule-table">
      <thead>
        <tr>
          <th scope="col">{t("perm.rules.permission")}</th>
          <th scope="col">{t("perm.rules.result")}</th>
          <th scope="col">{t("perm.rules.allow")}</th>
          <th scope="col">{t("perm.rules.deny")}</th>
        </tr>
      </thead>
      <tbody>
        {permissionsFor(root).map((name) => {
          const bit = PERM[name];
          const cell = result?.find((c) => c.permission === bit);
          const label = t(`perm.name.${name}`);
          return (
            <tr key={name}>
              <th scope="row">{label}</th>
              <td>
                <span
                  className="who-grid__cell"
                  data-verdict={cell?.verdict}
                  aria-label={cell === undefined ? undefined : t(`perm.verdict.${cell.verdict}`)}
                  role="img"
                >
                  {cell === undefined
                    ? ""
                    : { allowed: "✓", denied: "✗", depends: "?" }[cell.verdict]}
                </span>
              </td>
              {(["grant", "deny"] as const).map((side) => (
                <td key={side}>
                  <input
                    type="checkbox"
                    checked={(rule[side] & bit) !== 0}
                    disabled={!editable}
                    aria-label={t(`perm.rules.${side === "grant" ? "allowOf" : "denyOf"}`, {
                      permission: label,
                    })}
                    onChange={(e) => {
                      onToggle(bit, side, e.target.checked);
                    }}
                  />
                </td>
              ))}
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
