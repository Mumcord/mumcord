/**
 * "{names} are here without a camera" under the grid (FR-8.67): everyone else in the voice channel
 * without a camera, plain Mumble users included, with their stacked avatars.
 */
import { useTranslation } from "react-i18next";
import type { User } from "../../../api";
import { Avatar } from "../../members/Avatar";

/** More avatars than this make the stack wider than the sentence; the names still say all. */
const MAX_AVATARS = 5;

export function WithoutCamera({
  people,
  avatarOf,
}: {
  people: readonly User[];
  avatarOf: (user: User) => string | null;
}) {
  const { t } = useTranslation("camera");
  if (people.length === 0) return null;
  const names = people.map((user) => user.name).join(", ");
  return (
    <p className="cam-room__others">
      <span className="cam-room__stack">
        {people.slice(0, MAX_AVATARS).map((user) => (
          <Avatar key={user.session} name={user.name} src={avatarOf(user)} size="sm" />
        ))}
      </span>
      {t("room.without", { count: people.length, names })}
    </p>
  );
}
