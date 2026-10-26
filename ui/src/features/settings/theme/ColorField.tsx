import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { isHexColor } from "../../../theme/themeModel";

interface ColorFieldProps {
  label: string;
  value: string;
  /** Id of the contrast warning for this colour (FR-2.15), shown in the row heading. */
  issueId: string | undefined;
  onChange: (color: string) => void;
}

/**
 * One key colour of one version: the colour button opens the system colour picker; the hex field
 * shows the value and takes typed colours for keyboard and screen-reader use.
 */
export function ColorField({ label, value, issueId, onChange }: ColorFieldProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  // Text being typed that isn't a complete colour yet; the field shows the colour otherwise.
  const [typed, setTyped] = useState<string | null>(null);
  const invalid = typed !== null && !isHexColor(typed.trim().toLowerCase());
  const describedBy = [invalid && `${id}-invalid`, issueId].filter(Boolean).join(" ");
  return (
    <div className="theme-color">
      <input
        type="color"
        className="theme-color__picker"
        aria-label={t("themeEditor.pickColor", { color: label })}
        value={value}
        onChange={(event) => {
          setTyped(null);
          onChange(event.target.value.toLowerCase());
        }}
      />
      <input
        aria-label={label}
        className="settings-input theme-color__hex"
        value={typed ?? value}
        spellCheck={false}
        autoComplete="off"
        aria-invalid={invalid}
        aria-describedby={describedBy || undefined}
        onChange={(event) => {
          const text = event.target.value;
          const color = text.trim().toLowerCase();
          if (isHexColor(color)) {
            setTyped(null);
            onChange(color);
          } else {
            setTyped(text);
          }
        }}
      />
      {invalid && (
        <p id={`${id}-invalid`} className="settings-error theme-color__note">
          {t("themeEditor.invalidHex")}
        </p>
      )}
    </div>
  );
}
