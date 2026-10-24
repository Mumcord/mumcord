/**
 * Overlapping avatars of a channel's people on its row (QA.14): up to five plus a "+N" chip.
 * Read as one image whose label names the shown people, so the row's visible text stays a run of
 * its accessible name (the chip's digits and the initials are CSS-generated, WCAG 2.5.3).
 */
import { memo } from "react";
import { useTranslation } from "react-i18next";
import type { User } from "../../api";
import { Avatar } from "../members/Avatar";
import "./AvatarStack.css";

export interface StackPerson {
  user: User;
  talking: boolean;
  src: string | null;
}

export interface AvatarStackProps {
  /** The shown people, front first (see `stackPeople`). */
  people: readonly StackPerson[];
  /** Everyone in the channel, shown or not. */
  total: number;
}

function AvatarStackView({ people, total }: AvatarStackProps) {
  const { t } = useTranslation("channels");
  const rest = total - people.length;
  const names = people
    .map((p) => (p.talking ? t("tree.stack.talking", { name: p.user.name }) : p.user.name))
    .join(", ");
  const label =
    rest > 0
      ? t("tree.stack.some", { shown: people.length, total, names, rest })
      : t("tree.stack.all", { count: total, names });
  return (
    <>
      <span className="visually-hidden">, </span>
      <span className="avatar-stack" role="img" aria-label={label}>
        {people.map((p) => (
          <span key={p.user.session} className="avatar-stack__item">
            <Avatar name={p.user.name} src={p.src} talking={p.talking} size="sm" />
          </span>
        ))}
        {rest > 0 && <span className="avatar-stack__more" data-more={`+${String(rest)}`} />}
      </span>
    </>
  );
}

const samePeople = (a: AvatarStackProps, b: AvatarStackProps) =>
  a.total === b.total &&
  a.people.length === b.people.length &&
  a.people.every((p, i) => {
    const q = b.people[i];
    return q !== undefined && p.user === q.user && p.talking === q.talking && p.src === q.src;
  });

/** Re-renders only when the shown people, their talking state or images change. */
export const AvatarStack = memo(AvatarStackView, samePeople);
