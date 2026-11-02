import { useIndexImage } from "./useIndexImage";

/** The plugin's store icon, or its initial while there is none. Decorative: the name is next to it. */
export function PluginIcon({ id, name, hasIcon }: { id: string; name: string; hasIcon: boolean }) {
  const url = useIndexImage(id, hasIcon ? { kind: "icon" } : null);
  return url === null ? (
    <span className="plugin-browse-icon plugin-browse-initial" aria-hidden="true">
      {(name.trim()[0] ?? id[0] ?? "?").toUpperCase()}
    </span>
  ) : (
    <img className="plugin-browse-icon" src={url} alt="" />
  );
}
