import { useTranslation } from "react-i18next";
import { useDnd } from "../../dnd/useDnd";
import "../../dnd/dnd.css";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";

/**
 * Do Not Disturb (FR-3.111, FR-3.112): the switch, "Follow system Do Not Disturb" and whether
 * Mumcord can read the desktop's state at all.
 */
export function DndGroup() {
  const { t } = useTranslation("settings");
  const { state, setManual, setFollowSystem } = useDnd();
  if (!state) return null;
  const { desktop, on } = state.system;
  const readable = desktop !== null && on !== null;
  return (
    <Group title={t("dnd.title")} icon="moon">
      <Switch
        label={t("dnd.label")}
        description={t("dnd.description")}
        checked={state.manual}
        onChange={setManual}
      />
      <Switch
        label={t("dnd.follow")}
        description={t("dnd.followDescription")}
        checked={state.followSystem}
        onChange={setFollowSystem}
      />
      <p className="dnd-status" data-kind={readable ? "detected" : "unavailable"} role="status">
        <span className="dnd-status__tag">
          {readable ? t("dnd.detected") : t("dnd.unavailable")}
        </span>
        <span>
          {readable
            ? t(on ? "dnd.detectedOn" : "dnd.detectedOff", { desktop })
            : t("dnd.unavailableText")}
        </span>
      </p>
    </Group>
  );
}
