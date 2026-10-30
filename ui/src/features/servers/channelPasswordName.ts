import type { TFunction } from "i18next";
import type { SavedChannelPassword } from "../../api";

/**
 * The name a stored channel password is shown with (issue #146): the user's own label, else
 * the channels it was entered for ("Officers (Raid)"), else "Password N".
 */
export function channelPasswordName(
  t: TFunction<"servers">,
  saved: SavedChannelPassword,
  index: number,
  label: string = saved.label ?? "",
): string {
  if (label.trim() !== "") return label.trim();
  const channels = saved.channels.flatMap(({ path }) => {
    const name = path.at(-1);
    const parent = path.at(-2);
    if (name === undefined) return [];
    return [parent === undefined ? name : t("dialog.channelPasswordChannel", { name, parent })];
  });
  if (channels.length > 0) return channels.join(", ");
  return t("dialog.channelPassword", { number: index + 1 });
}
