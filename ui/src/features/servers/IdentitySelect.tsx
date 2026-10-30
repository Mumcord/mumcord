import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { Identity } from "../../api";
import { Select } from "../../components/Select";

/** Option value of "use the default identity"; never a real identity id. */
const USE_DEFAULT = "default";

interface IdentitySelectProps {
  label: string;
  /** The server's identity; `null` means "use the default". */
  value: number | null;
  identities: readonly Identity[];
  onChange: (identityId: number | null) => void;
  /** Hint under the dropdown; callers word it for their context. */
  hint?: string | undefined;
  hintId?: string | undefined;
}

/**
 * "Identity for this server" dropdown (issue #148): "Use default (…)" first, then every
 * identity as "label (certificate name)". The themed dropdown of FR-2.20, fully keyboard
 * operable.
 */
export function IdentitySelect({
  label,
  value,
  identities,
  onChange,
  hint,
  hintId,
}: IdentitySelectProps) {
  const { t } = useTranslation("servers");
  const id = useId();
  const fallback = identities.find((identity) => identity.isDefault);
  const known = value !== null && identities.some((identity) => identity.id === value);
  return (
    <div className="servers-form__field">
      <label htmlFor={id}>{label}</label>
      <Select
        id={id}
        value={known ? String(value) : USE_DEFAULT}
        options={[
          {
            value: USE_DEFAULT,
            label: t("identity.useDefault", { identity: fallback?.label ?? "" }),
          },
          ...identities.map((identity) => ({
            value: String(identity.id),
            label: t("identity.option", { label: identity.label, name: identity.name }),
          })),
        ]}
        onChange={(next) => {
          onChange(next === USE_DEFAULT ? null : Number(next));
        }}
      />
      {hint !== undefined && (
        <p id={hintId} className="servers-form__hint">
          {hint}
        </p>
      )}
    </div>
  );
}
