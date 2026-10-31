/**
 * Autocomplete list above the message input for `@` mentions (FR-1.51): `@channel` and `@all`
 * (issue #70) above the people. The input keeps the focus; the list is a listbox the input
 * points at with `aria-activedescendant` (WAI-ARIA combobox pattern). It floats over the chat,
 * so opening it never shifts the layout.
 */
import { useEffect, useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { User } from "../../api";
import { Avatar } from "../members/Avatar";
import { optionId, type GroupMention } from "./mentions";

export type MentionOption = { kind: "group"; group: GroupMention } | { kind: "user"; user: User };

export interface MentionPickerProps {
  id: string;
  options: readonly MentionOption[];
  active: number;
  avatarOf: (user: User) => string | null;
  /** The channel: who is in it (`@channel`) and who reads it, listeners included (`@all`). */
  channel: { name: string; members: number; readers: number };
  onPick: (option: MentionOption) => void;
}

interface Indexed {
  option: MentionOption;
  index: number;
}

export function MentionPicker({
  id,
  options,
  active,
  avatarOf,
  channel,
  onPick,
}: MentionPickerProps) {
  const { t } = useTranslation("chat");

  const activeRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const indexed = options.map((option, index) => ({ option, index }));
  const groups = indexed.filter(({ option }) => option.kind === "group");
  const people = indexed.filter(({ option }) => option.kind === "user");

  function row({ option, index }: Indexed): ReactNode {
    return (
      // The input keeps the focus and handles the keyboard (aria-activedescendant); the click is
      // the mouse path to the same action.
      // eslint-disable-next-line jsx-a11y/click-events-have-key-events
      <div
        key={option.kind === "group" ? option.group : option.user.session}
        ref={index === active ? activeRef : undefined}
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
          onPick(option);
        }}
      >
        {option.kind === "group" ? (
          <>
            <span className="mention-picker__keyword">@{option.group}</span>
            <span className="mention-picker__hint">
              {option.group === "channel"
                ? t("mention.channelHint", { channel: channel.name, count: channel.members })
                : t("mention.allHint", { count: channel.readers })}
            </span>
          </>
        ) : (
          <>
            <Avatar name={option.user.name} src={avatarOf(option.user)} size="sm" />
            <span className="mention-picker__name">{option.user.name}</span>
          </>
        )}
      </div>
    );
  }

  function section(key: "groups" | "people", heading: string, rows: Indexed[]): ReactNode {
    const headingId = `${id}-${key}`;
    return (
      <div role="group" aria-labelledby={headingId}>
        <div id={headingId} className="mention-picker__section" role="presentation">
          {heading}
        </div>
        {rows.map(row)}
      </div>
    );
  }

  return (
    <div className="mention-picker">
      <div id={id} className="mention-picker__list" role="listbox" aria-label={t("mention.list")}>
        {groups.length > 0 ? (
          <>
            {section("groups", t("mention.groupsHeading"), groups)}
            {people.length > 0 ? section("people", t("mention.peopleHeading"), people) : null}
          </>
        ) : (
          people.map(row)
        )}
      </div>
      {/* Screen readers already know how to use a combobox; this is for the eyes. */}
      <p className="mention-picker__keys" aria-hidden="true">
        {t("mention.keys")}
      </p>
    </div>
  );
}
