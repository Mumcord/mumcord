/**
 * Autocomplete list above the message input for `@` mentions (FR-1.51). The input keeps the
 * focus; the list is a listbox the input points at with `aria-activedescendant` (WAI-ARIA
 * combobox pattern). It floats over the chat, so opening it never shifts the layout.
 */
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { User } from "../../api";
import { Avatar } from "../members/Avatar";
import { optionId } from "./mentions";

export interface MentionPickerProps {
  id: string;
  candidates: readonly User[];
  active: number;
  avatarOf: (user: User) => string | null;
  onPick: (user: User) => void;
}

export function MentionPicker({ id, candidates, active, avatarOf, onPick }: MentionPickerProps) {
  const { t } = useTranslation("chat");
  const list = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // jsdom has no scrollIntoView.
    list.current?.children[active]?.scrollIntoView({ block: "nearest" });
  }, [active]);

  return (
    <div
      ref={list}
      id={id}
      className="mention-picker"
      role="listbox"
      aria-label={t("mention.list")}
    >
      {candidates.map((user, index) => (
        // The input keeps the focus and handles the keyboard (aria-activedescendant); the click is
        // the mouse path to the same action.
        // eslint-disable-next-line jsx-a11y/click-events-have-key-events
        <div
          key={user.session}
          id={optionId(id, index)}
          className="mention-picker__option"
          role="option"
          tabIndex={-1}
          aria-selected={index === active}
          onMouseDown={(event) => {
            // Keep the focus in the input.
            event.preventDefault();
          }}
          onClick={() => {
            onPick(user);
          }}
        >
          <Avatar name={user.name} src={avatarOf(user)} size="sm" />
          <span className="mention-picker__name">{user.name}</span>
        </div>
      ))}
    </div>
  );
}
