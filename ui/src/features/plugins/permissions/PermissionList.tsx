import { useTranslation } from "react-i18next";
import {
  isPermissionId,
  PERMISSION_RISK,
  permissionTextKey,
  type PermissionRisk,
  type RequestedPermission,
} from "./catalogue";
import { PERMISSION_ICONS, UNKNOWN_PERMISSION_ICON } from "./permissionIcons";
import { usePermissionText } from "./usePermissionText";
import "./permissions.css";

export interface PermissionListProps {
  requested: readonly RequestedPermission[];
  /** Ids of the optional permissions that are switched on. */
  enabled: ReadonlySet<string>;
  onToggle: (id: string, on: boolean) => void;
  /** Read-only view (e.g. plugin details): optional rows show their state but cannot be changed. */
  readOnly?: boolean;
  /** What the plugin's chat patterns recognise (its own words), shown with `chat.patterns`. */
  chatPatterns?: readonly string[];
}

function Row({
  id,
  details = [],
  toggle,
}: {
  id: string;
  /** Plugin-provided lines about this permission; rendered as plain text. */
  details?: readonly string[];
  toggle?: { checked: boolean; disabled: boolean; onChange: (on: boolean) => void };
}) {
  const { t } = useTranslation("pluginPermissions");
  const permissionText = usePermissionText();
  // Unknown ids cannot come from a validated manifest; show them as the highest risk, never hide them.
  const known = isPermissionId(id);
  const risk: PermissionRisk = known ? PERMISSION_RISK[id] : "red";
  const text = permissionText(id);
  const riskLabel = t(`risk.${risk}`);
  const content = (
    <>
      <span className={`perm-icon perm-risk-${risk}`} aria-hidden="true">
        {known ? PERMISSION_ICONS[id] : UNKNOWN_PERMISSION_ICON}
      </span>
      <span className="perm-body">
        <span className="perm-text">{text}</span>
        {known && <span className="perm-reason">{t(`explanations.${permissionTextKey(id)}`)}</span>}
        {details.map((line, i) => (
          <span key={i} className="perm-detail">
            {line}
          </span>
        ))}
      </span>
      {risk === "red" ? (
        <span className="perm-risk-badge">{riskLabel}</span>
      ) : (
        <span className="perm-sr-only">{riskLabel}</span>
      )}
    </>
  );
  if (toggle === undefined) {
    return (
      <li className="perm-row">
        {content}
        <span className="perm-chip">{t("required")}</span>
      </li>
    );
  }
  return (
    <li className="perm-row">
      <label className="perm-label">
        {content}
        <input
          type="checkbox"
          role="switch"
          className="perm-switch"
          checked={toggle.checked}
          disabled={toggle.disabled}
          onChange={(e) => {
            toggle.onChange(e.target.checked);
          }}
        />
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
  chatPatterns = [],
}: PermissionListProps) {
  const { t } = useTranslation("pluginPermissions");
  // Required rows first, then optional ones, as one list (spec mockup).
  const rows = [...requested.filter((p) => p.required), ...requested.filter((p) => !p.required)];
  const detailsOf = (id: string) => (id === "chat.patterns" ? chatPatterns : []);
  return (
    <ul className="perm-rows" aria-label={t("listLabel")}>
      {rows.map((p) =>
        p.required ? (
          <Row key={p.id} id={p.id} details={detailsOf(p.id)} />
        ) : (
          <Row
            key={p.id}
            id={p.id}
            details={detailsOf(p.id)}
            toggle={{
              checked: enabled.has(p.id),
              disabled: readOnly,
              onChange: (on) => {
                onToggle(p.id, on);
              },
            }}
          />
        ),
      )}
    </ul>
  );
}
