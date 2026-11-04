import { PluginTile } from "../render/PluginTile";
import { useIndexImage } from "./useIndexImage";

/** The plugin's store icon, or its colour tile while there is none. Decorative: the name is next to it. */
export function PluginIcon({ id, name, hasIcon }: { id: string; name: string; hasIcon: boolean }) {
  const url = useIndexImage(id, hasIcon ? { kind: "icon" } : null);
  return url === null ? (
    <PluginTile id={id} name={name} className="plugin-browse-icon plugin-browse-initial" />
  ) : (
    <img className="plugin-browse-icon" src={url} alt="" />
  );
}
