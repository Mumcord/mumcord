# Mumcord UI

React 19 + TypeScript (strict) inside Tauri. Vite root is this folder; `npm run dev` serves it on
port 1420 (`VITE_MUMCORD_MOCK=1` forces the in-memory engine mock). Engine access always goes
through `getApi()` from `src/api` (see `docs/engine-api.md`).

## Structure

| Folder          | Purpose                                                               |
| --------------- | --------------------------------------------------------------------- |
| `src/app/`      | App root, view state (wizard / main / settings overlay), shell layout |
| `src/theme/`    | Design tokens (`tokens.css`), global styles, theme resolution         |
| `src/i18n/`     | i18next setup and language selection                                  |
| `src/a11y/`     | Live-region announcer, skip links                                     |
| `src/locales/`  | Translation files `<language>/<namespace>.json`                       |
| `src/features/` | Feature UIs (servers, channels, chat, …), one folder per feature      |

## Mount points

The main window is a four-column shell (`app/AppShell.tsx`): server bar, channel column with the
user panel at the bottom, chat column (with an optional status bar above the chat), member column.
Each region is a landmark (`nav`, `main`, `aside`, `section`) with a translated label.

Features plug in through `app/mounts.tsx`: replace the placeholder body of your mount
(`ServersMount`, `ChannelsMount`, `UserPanelMount`, `ChatStatusMount`, `ChatMount`, `MembersMount`,
`WizardMount`, `SettingsMount`) with your feature's root component. Don't change the shell itself.

View state comes from `useView()` (`app/view.ts`): `screen` (`loading`, `startupError`, `wizard`,
`main`), `overlay` (`settings` or `null`), and `finishWizard()`, `openSettings()`,
`closeOverlay()`. The wizard is shown when there is no identity or `wizardCompleted` is false.

## Theme

- Use only the CSS variables from `theme/tokens.css` (`--color-*`, `--space-*`, `--radius-*`,
  `--font-size-*`, `--duration-*`); never hard-code colours. Sizes are in `rem` so the OS font size
  scales everything.
- Text colours (`--color-text`, `-text-secondary`, `-text-muted`, `-accent-text`, `-success`,
  `-warning`, `-danger`) are checked for 4.5:1 on every background token
  (`--color-bg-servers|sidebar|chat|elevated|hover`); `--color-on-accent` goes on `--color-accent`.
  New colour tokens must be added to both themes and to `theme/contrast.test.ts`.
- Focus styles are global (`:focus-visible`); don't remove outlines. Animations use
  `--duration-*`, which drop to 0 under reduced motion.
- After saving settings, pass the result to `usePreferences().applySettings(settings)`: theme and
  language then switch immediately (the engine sends no settings event).

## Translations

- Add `src/locales/{en,es,de}/<feature>.json`; files are picked up automatically, the namespace is
  the file name. Use `useTranslation("<feature>")` and nested keys (`t("header.follow")`).
- Shared words (OK, Cancel, Close, …) live in `common`. All three languages must have exactly the
  same keys; `i18n/i18n.test.ts` fails otherwise.

## Accessibility conventions (WCAG 2.2 AA)

- Everything is reachable with the keyboard in a logical Tab order: skip links → server bar →
  channels → user panel → chat → members.
- Inside lists and trees (servers, channels, members, messages) use a single Tab stop with
  roving `tabIndex` and arrow keys (Up/Down; Left/Right to collapse/expand tree nodes; Home/End).
- `Escape` closes overlays, dialogs and menus; focus returns to the element that opened them.
  Modal content gets `role="dialog"`, `aria-modal`, a labelled title, and the shell behind it is
  `inert`.
- Every control has an accessible name; icon-only buttons use `aria-label` or a
  `.visually-hidden` text.
- Events without visible focus change (talking, joins/leaves, new messages in other channels,
  connection loss) are spoken with `announce(message, "polite" | "assertive")` from `src/a11y`.
  Use "assertive" only for urgent problems.
- Every component test renders under both themes where colours matter and checks
  `axeViolations()` from `src/test/axe.ts`.
