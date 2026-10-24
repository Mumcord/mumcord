import type { Settings } from "../../../api";
import type { UpdateSettings } from "../useSettings";

/** Props of sections that edit the settings record. */
export interface SettingsSectionProps {
  settings: Settings;
  update: UpdateSettings;
}
