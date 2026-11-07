import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type AclCell, type AclRowInput, type ChannelAclView } from "../../api";
import {
  gridRows,
  nameOfBit,
  PERM,
  PERMISSION_GROUPS,
  ruleWho,
  userName,
} from "./permissionsModel";

interface WhoGridProps {
  serverId: number;
  acl: ChannelAclView;
  channelName: string;
  root: boolean;
  /** The person picked in "Check a person…", added as the last row (FR-10.49). */
  checked: AclRowInput | null;
  checkedName: string;
  /** "Go to rule N" in the Why? panel. */
  onGoToRule: (index: number) => void;
  /** Fewer columns, for the summary under the Groups view (mockup "acl-groups"). */
  columns?: readonly string[] | undefined;
}

/**
 * "Who can do what" (FR-10.40): one row per group or person named in the rules, one column per
 * permission. The answers come from the core and follow unsaved changes live; a cell opens the
 * Why? panel (FR-10.46–FR-10.48).
 */
export function WhoGrid({
  serverId,
  acl,
  channelName,
  root,
  checked,
  checkedName,
  onGoToRule,
  columns,
}: WhoGridProps) {
  const { t } = useTranslation("admin");
  const rows = gridRows(acl, checked);
  const [cells, setCells] = useState<AclCell[][] | null>(null);
  const [selected, setSelected] = useState<{ row: number; permission: number } | null>(null);

  useEffect(() => {
    let live = true;
    getApi()
      .channelAclEvaluate(
        serverId,
        acl,
        rows.map((r) => r.row),
      )
      .then((result) => {
        if (live) setCells(result);
      })
      .catch((error: unknown) => {
        console.error("Working out the permissions failed", error);
      });
    return () => {
      live = false;
    };
    // `rows` follows from `acl` and `checked`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverId, acl, checked]);

  const label = (index: number): string => {
    const row = rows[index]?.row;
    if (row === undefined) return "";
    switch (row.kind) {
      case "everyone":
        return t("perm.everyone");
      case "group":
        return row.name;
      case "user":
        return userName(acl, row.userId);
      case "session":
        return checkedName;
    }
  };

  const groups = PERMISSION_GROUPS.filter((g) => root || g.rootOnly !== true)
    .map((g) => ({
      ...g,
      perms: g.perms.filter((p) => columns === undefined || columns.includes(p)),
    }))
    .filter((g) => g.perms.length > 0);
  const cellOf = (row: number, permission: number) =>
    cells?.[row]?.find((c) => c.permission === permission);
  const chosen = selected === null ? undefined : cellOf(selected.row, selected.permission);

  return (
    <div className="who-grid">
      <div className="who-grid__table-wrap">
        <table className="who-grid__table">
          <caption className="visually-hidden">
            {t("perm.gridCaption", { channel: channelName })}
          </caption>
          <thead>
            <tr>
              <th scope="col">{t("perm.who")}</th>
              {groups.flatMap((g) =>
                g.perms.map((p, i) => (
                  <th key={p} scope="col" data-group-start={i === 0 || undefined}>
                    {i === 0 && columns === undefined && (
                      <span className="who-grid__group">{t(`perm.group.${g.id}`)}</span>
                    )}
                    {t(`perm.name.${p}`)}
                  </th>
                )),
              )}
            </tr>
          </thead>
          <tbody>
            {rows.map((r, rowIndex) => (
              <tr key={r.key}>
                <th scope="row">{label(rowIndex)}</th>
                {groups.flatMap((g) =>
                  g.perms.map((p) => {
                    const bit = PERM[p];
                    const cell = cellOf(rowIndex, bit);
                    const symbol =
                      cell === undefined
                        ? ""
                        : { allowed: "✓", denied: "✗", depends: "?" }[cell.verdict];
                    const inherited =
                      cell?.rule !== null &&
                      cell?.rule !== undefined &&
                      acl.rules[cell.rule]?.inherited === true;
                    const isSelected = selected?.row === rowIndex && selected.permission === bit;
                    return (
                      <td key={p}>
                        <button
                          type="button"
                          className="who-grid__cell"
                          data-verdict={cell?.verdict}
                          data-inherited={inherited || cell?.rule === null || undefined}
                          aria-pressed={isSelected}
                          aria-label={t("perm.cellLabel", {
                            who: label(rowIndex),
                            permission: t(`perm.name.${p}`),
                            verdict: cell === undefined ? "" : t(`perm.verdict.${cell.verdict}`),
                          })}
                          onClick={() => {
                            setSelected(isSelected ? null : { row: rowIndex, permission: bit });
                          }}
                        >
                          <span aria-hidden="true">{symbol}</span>
                        </button>
                      </td>
                    );
                  }),
                )}
              </tr>
            ))}
          </tbody>
        </table>
        <p className="channel-window__hint">{t("perm.legend")}</p>
      </div>
      {chosen !== undefined && selected !== null && (
        <WhyPanel
          acl={acl}
          cell={chosen}
          who={label(selected.row)}
          channelName={channelName}
          onGoToRule={onGoToRule}
        />
      )}
    </div>
  );
}

/** The Why? panel (FR-10.46–FR-10.48): the deciding rule, why it matched, what it overrode. */
function WhyPanel({
  acl,
  cell,
  who,
  channelName,
  onGoToRule,
}: {
  acl: ChannelAclView;
  cell: AclCell;
  who: string;
  channelName: string;
  onGoToRule: (index: number) => void;
}) {
  const { t } = useTranslation("admin");
  const permission = t(`perm.name.${nameOfBit(cell.permission) ?? "write"}`);
  const rule = cell.rule === null ? undefined : acl.rules[cell.rule];
  const ruleText = (index: number) => {
    const r = acl.rules[index];
    if (r === undefined) return "";
    return t(r.inherited ? "perm.why.ruleInherited" : "perm.why.ruleHere", {
      n: index + 1,
      who: ruleWho(acl, r),
    });
  };
  let reason = "";
  if (cell.reason !== null) {
    reason =
      cell.reason.kind === "member"
        ? t("perm.why.reason.member", { who, group: cell.reason.group })
        : t(`perm.why.reason.${cell.reason.kind}`, { who, channel: channelName });
  }
  let condition = "";
  if (cell.condition !== null) {
    condition =
      cell.condition.kind === "password"
        ? t("perm.why.condition.password")
        : cell.condition.kind === "presence"
          ? t("perm.why.condition.presence", { channel: channelName })
          : t("perm.why.condition.serverGroup");
  }

  return (
    <aside className="why-panel" aria-label={t("perm.why.title")} aria-live="polite">
      <h3>{t("perm.why.title")}</h3>
      <p className="why-panel__headline">
        {t(`perm.why.headline.${cell.verdict}`, { who, permission, channel: channelName })}
      </p>
      {cell.superuser && <p>{t("perm.why.superuser")}</p>}
      {!cell.superuser && rule === undefined && !cell.viaWrite && <p>{t("perm.why.default")}</p>}
      {cell.viaWrite && <p>{t("perm.why.viaWrite")}</p>}
      {rule !== undefined && cell.rule !== null && (
        <p>{t("perm.why.decidedBy", { rule: ruleText(cell.rule) })}</p>
      )}
      {reason !== "" && <p>{reason}</p>}
      {condition !== "" && <p>{condition}</p>}
      {cell.overrides !== null && (
        <p>{t("perm.why.overrides", { rule: ruleText(cell.overrides) })}</p>
      )}
      {cell.rule !== null && (
        <button
          type="button"
          className="button"
          onClick={() => {
            if (cell.rule !== null) onGoToRule(cell.rule);
          }}
        >
          {t("perm.why.goTo", { n: cell.rule + 1 })}
        </button>
      )}
      <p className="channel-window__hint">{t("perm.why.footer")}</p>
    </aside>
  );
}
