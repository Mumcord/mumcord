import { useTranslation } from "react-i18next";
import {
  isPermissionId,
  PERMISSION_RISK,
  permissionTextKey,
  type PermissionRisk,
  type RequestedPermission,
} from "./catalogue";
import "./permissions.css";

export interface PermissionListProps {
  requested: readonly RequestedPermission[];
  /** Ids of the optional permissions that are switched on. */
  enabled: ReadonlySet<string>;
  onToggle: (id: string, on: boolean) => void;
  /** Read-only view (e.g. plugin details): optional rows show their state but cannot be changed. */
  readOnly?: boolean;
}

const RISK_ICON: Record<PermissionRisk, string> = { green: "●", yellow: "●", red: "▲" };

function Row({
  id,
  toggle,
}: {
  id: string;
  toggle?: { checked: boolean; disabled: boolean; onChange: (on: boolean) => void };
}) {
  const { t } = useTranslation("pluginPermissions");
  // Unknown ids cannot come from a validated manifest; show them as the highest risk, never hide them.
  const known = isPermissionId(id);
  const risk: PermissionRisk = known ? PERMISSION_RISK[id] : "red";
  const text = known ? t(`texts.${permissionTextKey(id)}`) : id;
  const riskLabel = t(`risk.${risk}`);
  const content = (
    <>
      <span className={`perm-risk perm-risk-${risk}`} aria-hidden="true">
        {RISK_ICON[risk]}
      </span>
      <span className="perm-text">{text}</span>
      {risk === "red" ? (
        <span className="perm-risk-badge">{riskLabel}</span>
      ) : (
        <span className="perm-sr-only">{riskLabel}</span>
      )}
    </>
  );
  if (toggle === undefined) {
    return <li className="perm-row">{content}</li>;
  }
  return (
    <li className="perm-row">
      <label className="perm-label">
        <input
          type="checkbox"
          checked={toggle.checked}
          disabled={toggle.disabled}
          onChange={(e) => {
            toggle.onChange(e.target.checked);
          }}
        />
        {content}
      </label>
    </li>
  );
}

/**
 * The permission list of the install prompt and plugin details (FR-6.60, §5.3): required rows are
 * fixed, optional rows are toggles. Risk is shown by colour, a symbol and a text, never colour alone.
 */
export function PermissionList({
  requested,
  enabled,
  onToggle,
  readOnly = false,
}: PermissionListProps) {
  const { t } = useTranslation("pluginPermissions");
  const required = requested.filter((p) => p.required);
  const optional = requested.filter((p) => !p.required);
  return (
    <div className="perm-list">
      {required.length > 0 ? (
        <section>
          <h3 className="perm-heading">{t("required")}</h3>
          <ul className="perm-rows">
            {required.map((p) => (
              <Row key={p.id} id={p.id} />
            ))}
          </ul>
        </section>
      ) : null}
      {optional.length > 0 ? (
        <section>
          <h3 className="perm-heading">{t("optional")}</h3>
          <ul className="perm-rows">
            {optional.map((p) => (
              <Row
                key={p.id}
                id={p.id}
                toggle={{
                  checked: enabled.has(p.id),
                  disabled: readOnly,
                  onChange: (on) => {
                    onToggle(p.id, on);
                  },
                }}
              />
            ))}
          </ul>
        </section>
      ) : null}
    </div>
  );
}
