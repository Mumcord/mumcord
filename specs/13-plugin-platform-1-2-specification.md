# 0.8.0 – Plugin platform 1.2 (theme tokens and UI kit) · Specification

> Status: Final (written directly from issue #691 and the product-owner decisions given with the task, no brainstorming file) · As of: 2026-11-08
> Sources: issue #691 (Mumcord theme tokens and UI kit for plugin web pages), product-owner decisions of 2026-11-08 (plugin API 1.2, `--mc-` tokens, reserved kit path, opt-in kit, Whiteboard migration after 0.8.0).
> Builds on: `specs/06-plugin-system-specification.md` (theme FR-6.23, web view and message bridge FR-6.47, manifest `api` FR-6.11), `specs/09-plugin-platform-1-1-specification.md` (API versions FR-9.5 … FR-9.13, registry indexes and version choice FR-9.14 … FR-9.21, web content host v2 FR-9.32 … FR-9.45, macOS child web view FR-9.3, SDK and docs FR-9.90 … FR-9.92), `specs/02-multi-server-themes-specification.md` (app themes, custom themes, FR-2.14 … FR-2.19).
> Repositories: `Mumcord/mumcord` (client, plugin host, SDK, `mumcord-plugin` CLI, platform docs), `Mumcord/plugins` (registry CI, author docs, template plugin, Whiteboard).
> Numbering: requirements and criteria use the spec file number (`FR-13.n`, `AC-13.n`). Spec number 12 is taken by `12-ssd-write-reduction-specification.md`. Where this spec and the issue differ, this spec wins.

## 1. Goal & context

Plugin web pages (stage, side panel, dialog, panel popover; spec 09) get no styling from Mumcord today: no theme colours, no CSS variables, no shared controls and no signal when the user changes the theme. Every plugin that wants to look like Mumcord copies Mumcord's design by hand, and the copies drift as soon as Mumcord's design changes. The official Whiteboard plugin (#397) shows the problem: its Export dialog uses plain radio dots, grey native dropdowns and a generic switch that don't look like Mumcord 0.8.0's own controls. The only theme access today is for the plugin's WebAssembly core (FR-6.23 `theme.current()`, token names only), which a page would have to forward over the message bridge itself.

0.8.0 adds **plugin API 1.2**, a small additive minor over 1.1 with two parts:

1. **Theme tokens in the page.** Mumcord writes its design tokens (colours, fonts, sizes, spacing, radii, focus ring, motion) as CSS variables with the prefix `--mc-` onto the root element of every page of a 1.2 plugin, together with the current mode (dark, light, high contrast). When the user changes the theme, the values change live without a reload and the page gets a DOM event.
2. **The Mumcord UI kit.** A stylesheet plus Mumcord's two fonts, shipped inside Mumcord and served to the page from the plugin's own address at a reserved path. A page opts in by linking it. It styles plain HTML the Mumcord way: buttons, radio buttons, checkboxes, switches, dropdowns, text fields, in-page dialogs, section labels and the focus ring.

In plain words: a plugin author writes ordinary HTML, links one file, and the page looks like Mumcord in every theme and keeps up with Mumcord's design. Nothing is downloaded from the internet and nothing changes on the Mumble server.

## 2. Scope

### 2.1 In scope

- **Plugin API 1.2**: WIT package `mumcord:plugin@1.2.0` (same interfaces as 1.1.0, new version only; FR-13.2), frozen snapshot, SDK feature `1.2`, manifest `api: "^1.2"`, supported by Mumcord 0.8.0 and later.
- **Registry and version choice** for 1.2 versions following spec 09: only in the `v1.1` index, `min_mumcord` 0.8 by default; older Mumcord never offers them.
- **Theme tokens** (`--mc-…`) and mode attributes on the root element of every page of a 1.2 plugin, present before the page's own code runs, updated live, with a `mumcord-theme-change` event.
- **UI kit** `kit.css` plus the fonts Instrument Sans and Bricolage Grotesque, served at the reserved path `_mumcord/v1/` inside the plugin's own web folder address. Opt-in, scoped, CSS only.
- **Reserved path rules**: plugin files can never take the reserved path's place.
- **Docs**: platform docs in Mumcord `docs/plugins/`, author docs in `Mumcord/plugins` (`docs/api.md`, new page "Styling your page" with an example), the template plugin and `mumcord-plugin new`; CHANGELOG entry.
- **Test plugin** `test-kit` and E2E for the platform in Mumcord.

### 2.2 Out of scope / later

- **New WIT functions.** The kit is pure web; the plugin core's `theme.current()` (FR-6.23) stays as it is, including its token names and its light/dark mode.
- **New token names for the plugin core's UI trees** (tile, panel and card trees, FR-6.41). The tree renderer keeps the nine FR-6.23 tokens.
- **JavaScript components.** The kit is a stylesheet; there is no kit script, no custom elements and no framework. Dropdown option lists stay the system's own list (FR-13.24).
- **Theming of pages of 1.0 and 1.1 plugins.** They stay exactly as they are (FR-13.8).
- **Remote stylesheets or fonts** (CSP stays unchanged, FR-9.33).
- **A UI scale or font-size setting in Mumcord.** Mumcord has none; system scaling and the web view zoom already apply to plugin frames, and the kit sizes everything in `rem` so it follows them.
- **Moving the official plugins in this repository.** Whiteboard moves to the kit in a separate `Mumcord/plugins` pull request (section 14). Other official plugins with web pages may follow later, each in its own plugins-repo change.
- **Any server-side change.**

## 3. Roles & permissions

| Role | Action | Enforced by |
|---|---|---|
| Mumcord user | Picks the app theme (dark, light, system, custom themes) and turns on OS high contrast as today; plugin pages follow | Client |
| Plugin author | Declares `api: "^1.2"` (through SDK feature `1.2`), links the kit, uses the `--mc-` tokens | `mumcord-plugin build`/`check`, registry CI, install checks |
| Plugin with `api` 1.2 | Its pages receive tokens, mode attributes and the theme event; `_mumcord/v1/` serves the kit | Plugin host (custom protocol, bootstrap) |
| Plugin with `api` 1.0 or 1.1 | Unchanged; no tokens, no event, the reserved path answers "not found" | Plugin host |
| Registry maintainers | Publish 1.2 versions only in `v1.1/index.json` with `min_mumcord` ≥ 0.8 | Registry CI |
| Mumble server admin | Nothing new | — |

No new permission. The kit and the tokens are visual only; they give the page no new reach (FR-13.30).

## 4. Functional requirements

### 4.1 API 1.2 and versioning

- **FR-13.1** Mumcord 0.8.0 supports the API minors `1.0`, `1.1` and `1.2` (extends FR-9.5). 1.0 and 1.1 plugins keep working unchanged.
- **FR-13.2** **WIT `mumcord:plugin@1.2.0`** contains exactly the interfaces, functions, events and types of `1.1.0`; only the package version differs. The host links it as a third package next to 1.0.0 and 1.1.0 in the same linker. The frozen snapshot `wit-1.2.0.snapshot` joins the existing ones and the snapshot test (FR-9.7) covers it. Reason: the rule "a version's `api` matches the WIT its component imports" (FR-9.20, FR-9.21) stays true without a special case, and the host and registry can tell 1.2 plugins apart.
- **FR-13.3** **SDK feature `1.2`** in `crates/plugin-sdk` builds against `wit/1.2.0` and includes everything of feature `1.1`. `mumcord-plugin build` writes `api: "^1.2"` for it (FR-9.6); the author never types it. `mumcord-plugin new` creates 1.2 plugins by default.
- **FR-13.4** **Contract**: the token names of section 4.3, the mode attribute values and event name of section 4.4, and the kit's class names, element coverage and reserved paths of sections 4.5 and 4.6 are part of plugin API 1.x. Within 1.x they are only ever added to, never renamed or removed (FR-9.7, FR-9.11). A later minor that adds tokens or kit classes gives them only to plugins declaring that minor or newer.
- **FR-13.5** **Values are not part of the contract.** Colours, sizes and the kit's exact look follow Mumcord's design and may change in any release; that is the point of the kit.

### 4.2 Registry and version choice

- **FR-13.6** Index format 2 (FR-9.15) carries `api: "1.2"` unchanged; the default `min_mumcord` for `api` 1.2 is `0.8`. Registry CI (`Mumcord/plugins`) accepts `api` 1.2, lists 1.2 versions only in `v1.1/index.json`, never in `v1/index.json`, and rejects a 1.2 version with `min_mumcord` lower than `0.8` (FR-9.20). `mumcord-plugin site-data` follows the same rules.
- **FR-13.7** Version choice, install refusal and update hold work as in FR-9.16 … FR-9.18 with no change: Mumcord 0.7.x does not support `1.2`, so it keeps a plugin on its newest 1.0/1.1 version, shows "Needs Mumcord 0.8.0 or newer" for plugins that only have 1.2 versions, and refuses local or third-party 1.2 packages before writing anything to disk. `docs/plugin-registry-format.md` and the third-party repository docs gain the `1.2 → 0.8` default.

### 4.3 Theme tokens in the page

- **FR-13.8** **Only 1.2 plugins.** Tokens, mode attributes and the theme event (sections 4.3 and 4.4) are given to every page of a plugin whose `api` is 1.2 or newer, in every place (stage, pop-out, side panel, dialog, panel popover), in both frame transports (iframe and the macOS child web view, FR-9.3). Pages of 1.0 and 1.1 plugins get none of it and behave byte for byte as in 0.7.x.
- **FR-13.9** **Token set (API 1.2).** Mumcord sets these custom properties on the page's root element (`<html>`). Colour tokens carry the current resolved colour of the app theme, including custom themes and their derived colours (FR-2.14); all others carry Mumcord's current values. The nine FR-6.23 names appear unchanged after `--mc-color-`, so the plugin core and its page use the same words.

| Group | Tokens | Taken from (Mumcord, informative) |
|---|---|---|
| Colours (FR-6.23 names) | `--mc-color-text`, `--mc-color-text-muted`, `--mc-color-accent`, `--mc-color-danger`, `--mc-color-warning`, `--mc-color-success`, `--mc-color-surface`, `--mc-color-surface-raised`, `--mc-color-border` | `--color-text`, `--color-text-muted`, `--color-accent`, `--color-danger`, `--color-warning`, `--color-success`, `--color-bg-chat`, `--color-bg-elevated`, `--color-border` |
| Colours (additional) | `--mc-color-text-secondary`, `--mc-color-accent-text`, `--mc-color-on-accent`, `--mc-color-primary`, `--mc-color-on-primary`, `--mc-color-hover`, `--mc-color-selected`, `--mc-color-border-strong`, `--mc-color-focus`, `--mc-color-backdrop` | `--color-text-secondary`, `--color-accent-text`, `--color-on-accent`, `--color-primary-bg`, `--color-primary-text`, `--color-bg-hover`, `--color-bg-selected`, `--color-border-strong`, `--color-focus`, `--color-backdrop` |
| Type | `--mc-font-family`, `--mc-font-family-display`, `--mc-font-family-mono`, `--mc-font-size-xs`, `--mc-font-size-sm`, `--mc-font-size-md`, `--mc-font-size-lg`, `--mc-font-size-xl`, `--mc-font-weight-normal`, `--mc-font-weight-bold`, `--mc-line-height` | same names without `mc-` |
| Space and shape | `--mc-space-1` … `--mc-space-6`, `--mc-radius-sm`, `--mc-radius-md`, `--mc-radius-lg`, `--mc-radius-full`, `--mc-focus-ring-width`, `--mc-shadow-raised` | same names without `mc-` |
| Motion | `--mc-duration-fast`, `--mc-duration-normal`, `--mc-ease-out` | same names without `mc-`; durations are `0ms` while reduced motion is on (FR-13.12) |

- **FR-13.10** **Font families** in the tokens name the bundled faces (`"Instrument Sans Variable"`, `"Bricolage Grotesque Variable"`) first, then the same system fallbacks as Mumcord. The faces themselves are only available to a page that links the kit (FR-13.18); without it the page falls back to the system font.
- **FR-13.11** **Mode attributes** on the root element: `data-mc-mode` is `dark`, `light` or `high-contrast`; `high-contrast` is set while the operating system's forced-colours / high-contrast mode is active in Mumcord's window, otherwise the resolved app variant (dark or light). `data-mc-scheme` is always `dark` or `light` (the base variant, also under high contrast). Mumcord sets the CSS `color-scheme` of the root to the same value, so native scrollbars and form controls match.
- **FR-13.12** **Reduced motion**: `data-mc-reduced-motion` is `true` or `false`, following the same value the plugin core sees (FR-6.23 `reduced-motion`: the OS setting or Mumcord's "visual effects off"). While `true`, `--mc-duration-fast` and `--mc-duration-normal` are `0ms`.
- **FR-13.13** **No flash.** Tokens and attributes are on the root element before the first script of the page runs and before first paint, so a page never shows unthemed colours. Recommended mechanism: when serving an HTML page of a 1.2 plugin, the custom protocol places the current values as a JSON data block (`<script type="application/json">`, never executed) right before the injected bootstrap (FR-9.41); the bootstrap applies them and removes the block. The UI hands the plugin service the resolved values on every theme change, as it already reports mode and reduced motion (FR-6.23). Any mechanism is acceptable if AC-13.3 passes.
- **FR-13.14** **Escaping.** Values embedded into the served HTML are JSON-escaped so they can never close the data block (`<`, `>`, `&`, U+2028 and U+2029 escaped). Token values are only ever Mumcord's own values or the colours of the user's themes, which are validated hex colours (FR-2.14); anything else is dropped and the token is left out rather than passed through.

### 4.4 Live theme changes

- **FR-13.15** When the theme changes while a 1.2 page is open (app theme, dark/light variant, system scheme under "system", custom theme edit or preview, OS high contrast on/off, reduced motion or visual effects), Mumcord updates all tokens and attributes of every open 1.2 page within 200 ms, without reloading the page and without touching the page's own state.
- **FR-13.16** After applying the new values, the bootstrap dispatches a `CustomEvent` named `mumcord-theme-change` on `window`, with `detail` `{ mode, scheme, reducedMotion }` (the same values as the attributes; `reducedMotion` a boolean). It is not dispatched on first load; the initial values are already present (FR-13.13). Pages that use only CSS variables need no listener; the event is for canvas drawing and similar code that reads colours once.
- **FR-13.17** **Transport.** Theme updates travel from the app to the frame like the bootstrap's other host messages: an object with the `mumcordWebview` key, never a string, so they never reach the page's bridge `message` handler as plugin data and never count against the bridge limits (FR-6.47, FR-9.37). The bootstrap accepts them only from its host (the same source check as for mute requests). A page that changes the `--mc-` properties itself only changes its own look.

### 4.5 The UI kit

- **FR-13.18** **Files.** Mumcord ships the kit inside the app: `kit.css` and the font files (Instrument Sans Variable and Bricolage Grotesque Variable, Latin subsets, `woff2`, the same faces the app bundles, OFL-1.1) plus their licence text. Nothing is fetched from the network, ever.
- **FR-13.19** **Reserved path.** For a 1.2 plugin, the custom protocol serves the kit at `_mumcord/v1/` inside the plugin's own web address, next to its `web/` files: `mumcord-web://localhost/<plugin>/_mumcord/v1/kit.css`, `…/_mumcord/v1/fonts/<file>.woff2`, `…/_mumcord/v1/fonts/OFL.txt`. A page links it relatively, e.g. `<link rel="stylesheet" href="_mumcord/v1/kit.css">` from a page at the root of `web/`. Because this is the plugin's own address, the existing frame policy already allows it (`style-src` and `font-src` own package, FR-9.33); the CSP does not change. The `v1` segment is the API major; a future 2.0 kit would live at `_mumcord/v2/` next to it.
- **FR-13.20** **Opt-in and scope.** Linking `kit.css` alone changes nothing on the page except loading the font faces. The kit styles only:
  - elements with a kit class (`mc-` prefix, section 4.6), anywhere on the page, and
  - native controls (`button`, `input`, `select`, `textarea`, `label`, `fieldset`, `legend`) inside an element with the class `mc-kit` (typically `<body class="mc-kit">`).
  All kit rules use `:where()` so their specificity is zero and any style of the page wins. The kit never styles `html`, `body` or `*` on its own and never uses `!important` (except inside `@media (forced-colors: active)`, where it only restores system colours). A page that does not link the kit is unchanged (AC-13.8).
- **FR-13.21** **Tokens only.** Every colour, size, radius, font and duration in `kit.css` comes from the `--mc-` tokens of FR-13.9, so the kit follows theme changes without being reloaded. `kit.css` itself is the same file for every theme.
- **FR-13.22** **Same source as the app.** The kit's controls are built from the same design source as Mumcord's own controls (`ui/src/theme/controls.css` radio and checkbox, the pill buttons in `ui/src/app/shell.css`, the switch in Settings, the trigger of `ui/src/components/Select.tsx`, the app's text fields, dialogs and section labels), so a change to Mumcord's controls changes the kit in the same pull request. A test fails if a kit control's computed look differs from the app's control in any theme (AC-13.9).
- **FR-13.23** **Served like plugin files, never from them.** Kit responses carry the same frame policy and Permissions-Policy headers as every `web/` response (FR-9.33, FR-9.34) and the right MIME type (`text/css`, `font/woff2`, `text/plain`). Pages of 1.0 and 1.1 plugins get "not found" for every path under `_mumcord/`.

### 4.6 Kit coverage (API 1.2)

- **FR-13.24** The kit styles, each in rest, hover, active, focus-visible, disabled and (where it applies) checked / invalid state, in dark, light and high contrast:

| Element | How the page uses it | Looks like (Mumcord 0.8.0) |
|---|---|---|
| Buttons | `<button class="mc-button">` (secondary, default), `mc-button mc-button--primary`, `mc-button mc-button--danger`; inside `.mc-kit` a plain `<button>` is a secondary button | Pill buttons of `shell.css` (`.button`, `.button--primary`), danger in the danger colour |
| Radio button | `<input type="radio">` inside `.mc-kit`, or with class `mc-radio` | Ring with accent centre (`controls.css`) |
| Checkbox | `<input type="checkbox">` inside `.mc-kit`, or with class `mc-checkbox` | Rounded square with accent check (`controls.css`) |
| Switch | `<input type="checkbox" role="switch">` inside `.mc-kit`, or with class `mc-switch` | The Settings switch |
| Dropdown | `<select>` inside `.mc-kit`, or with class `mc-select` | The closed `Select` trigger (field, chevron, radius); the open list is the system list |
| Text field | `<input type="text|search|number|email|url|password">` and `<textarea>` inside `.mc-kit`, or with class `mc-input` / `mc-textarea` | The app's text fields, invalid state via `aria-invalid="true"` |
| Field label and hint | `<label class="mc-label">`, `<p class="mc-hint">` | Settings field label and muted hint |
| Section label | `<h2 class="mc-section-label">` (any heading level) | Settings section heading |
| Dialog chrome | `.mc-dialog` with `.mc-dialog__header`, `.mc-dialog__title`, `.mc-dialog__body`, `.mc-dialog__footer`; works on a `<div role="dialog">` and on `<dialog>`; `.mc-backdrop` for a dimmed backdrop | Mumcord's dialogs: raised surface, radius, shadow, title in the display font, buttons right-aligned in the footer |
| Focus ring | Every kit control and any element with class `mc-focus-ring` | Mumcord's focus ring (`--mc-color-focus`, `--mc-focus-ring-width`, on `:focus-visible` only) |
| Surface and text | `.mc-surface`, `.mc-surface-raised`, `.mc-text-muted` | Chat and raised surfaces, muted text |

- **FR-13.25** **Controls stay native.** The kit restyles the elements above with CSS only; their keyboard behaviour, form semantics and screen-reader roles stay the browser's. The kit never hides a control in a way that removes it from the accessibility tree.
- **FR-13.26** **High contrast.** Under `@media (forced-colors: active)` every kit control uses system colours (`ButtonText`, `ButtonFace`, `Highlight`, `HighlightText`, `GrayText`, `CanvasText`, `Canvas`) and keeps a visible border and focus ring, like Mumcord's own controls.
- **FR-13.27** **Scaling.** All kit sizes are in `rem` or `em`, so the kit follows system scaling and the web view zoom like Mumcord itself.

### 4.7 Reserved path protection

- **FR-13.28** The reserved prefix is the first path segment `_mumcord`, compared without regard to ASCII case. The custom protocol answers every request under it from Mumcord's own kit files (1.2+) or with "not found" (1.0, 1.1); it **never** reads a file of the plugin package for such a path, for any plugin.
- **FR-13.29** A 1.2 package whose `web/` folder contains anything under `_mumcord` (any case) is rejected by `mumcord-plugin build` and `check`, by registry CI and at install time (before anything is written to disk, with the existing "package is invalid" text and the reason "uses the reserved folder `web/_mumcord`"). For 1.0 and 1.1 packages `mumcord-plugin check` warns; such files are simply never served (FR-13.28). A page entry (`ui.webview` `show`, stage entry, manifest places) under `_mumcord` is invalid for every plugin (`parse_entry` refuses it).
- **FR-13.30** The kit adds no capability: no script, no network, no storage, no new bridge message. The frame's sandbox, CSP, Permissions-Policy, heartbeat, watchdog and limits (FR-9.32 … FR-9.45) are unchanged for every plugin.

### 4.8 SDK, CLI, docs and template

- **FR-13.31** **Mumcord docs**: new `docs/plugins/api-1-2.md` (what 1.2 adds, the token table, mode attributes, event, kit paths and classes, reserved path rules, the contract of FR-13.4/FR-13.5, `min_mumcord` 0.8); `docs/plugins/api-1-1.md` links to it; `docs/plugin-registry-format.md` gains the `1.2 → 0.8` default; spec 06 FR-6.23 and FR-6.47 and spec 09 §4.6 get a pointer to this spec.
- **FR-13.32** **`Mumcord/plugins` docs**: `docs/api.md` gains the API 1.2 section; a new page `docs/styling-your-page.md` ("Styling your page", added to the `mkdocs.yml` navigation) explains opt-in, the scope class, every kit element with a copyable snippet, the tokens, reacting to `mumcord-theme-change` from canvas code, high contrast, and contains one complete small example page (a settings-style form with a dialog). `docs/publishing.md` and `docs/third-party-repositories.md` state that 1.2 versions go only into the `v1.1` index with `min_mumcord` 0.8.
- **FR-13.33** **Template**: the `template/` plugin in `Mumcord/plugins` and the web-content template of `mumcord-plugin new` use SDK feature `1.2`, link the kit and use only kit classes and tokens for their page, so a new plugin looks like Mumcord from the start.
- **FR-13.34** **CHANGELOG**: an `[Unreleased]` → `Added` entry in Mumcord: "Plugin API 1.2: plugin web pages follow your Mumcord theme (dark, light, high contrast, live changes) and can use Mumcord's own controls through the built-in UI kit."

## 5. UI & UX

- There is no new Mumcord screen and no new text in the app. The visible result is inside plugin pages: they match Mumcord's colours, fonts and controls and follow theme changes immediately.
- Mumcord's chrome around a frame (FR-9.36: plugin icon, name, "Unofficial" chip, "Plugin content" tag) is unchanged. Looking like Mumcord does not make plugin content Mumcord content; the tag stays the honest signal (see section 9).
- **Accessibility** (applies to the kit and the token behaviour):
  - Keyboard: every kit control keeps native keyboard behaviour; the focus ring shows on `:focus-visible` with at least 3:1 contrast against its surface in every theme.
  - Screen readers: native roles and states stay intact (FR-13.25); the switch relies on `role="switch"` and `checked`; dialog chrome is visual only, so the docs example shows `role="dialog"`, `aria-modal`, `aria-labelledby` and focus handling.
  - Contrast: kit text and control borders use token pairs that Mumcord's contrast test already checks (≥ 4.5:1 for text, ≥ 3:1 for control borders and focus ring) in dark, light and custom themes.
  - High contrast (FR-13.26), system scaling (FR-13.27) and reduced motion (FR-13.12) work without any code in the page.

## 6. Audio & network

Nothing new. The kit and the tokens never touch audio, the Mumble connection, the relay or the network; the fonts are local files.

## 7. Data & state model

- **No stored data.** No migration, no new settings.
- **In memory (Rust, `crates/plugins`)**: the theme state of FR-6.23 (`Theme`, mode and reduced motion) grows by the resolved token values and the high-contrast flag, set by the UI on every theme change, read by the custom protocol when it serves an HTML page of a 1.2 plugin.
- **In the app (UI)**: the list of open 1.2 frames already known to `PluginFrame`/`childFrame`; on a theme change each receives one theme host message.
- **Shipped files**: `kit.css`, two `woff2` files and `OFL.txt`, bundled into the app's resources and served from memory or the resource folder.

## 8. Technology & architecture

- **Kit source in the UI tree** (`ui/src/plugin-kit/`: `kit.css` written against `--mc-` tokens), built and bundled with the app; the Rust side serves the bundled files. Reason: the kit sits next to the app's own controls, so one pull request changes both, and the computed-style test (AC-13.9) keeps them equal.
- **Token mapping in one place** (a TypeScript table next to `render/tokens.ts`, `--mc-<name>` → app variable) used both for the initial values and for live updates. Reason: one source of truth; a unit test checks it against FR-13.9 and fails on removals.
- **Live updates through the bootstrap** (`webview_bootstrap.js`), as host messages like mute. Reason: the bootstrap already runs first in every page, already trusts only its host, and is allowed by its CSP hash; no new inline script and no CSP change. The bootstrap stays one file for all plugins; the app simply sends theme messages only to 1.2 frames, and the protocol only embeds initial values for 1.2 pages.
- **Reserved path in the custom protocol handler** (`src-tauri/src/plugins/webview.rs` with the rules in `crates/plugins/src/api/webview.rs`). Reason: the protocol already checks every path (`is_web_path`); checking the reserved prefix before the file lookup is a single early branch.
- **WIT 1.2.0 as a copy of 1.1.0** with a new package version, linked like 1.1.0 (FR-9.8 conversion not needed: identical types). Reason: keeps every existing versioning rule intact with no special cases.
- Module boundaries stay: protocol, audio, plugin host, UI; the UI never talks to the network.

## 9. Non-functional requirements

- **Security**: no change to sandbox, CSP or Permissions-Policy (FR-13.30); reserved path never served from packages (FR-13.28); embedded values escaped (FR-13.14); the kit is CSS only. Because the kit makes plugin pages look like Mumcord, the frame chrome and the "Plugin content · <plugin>" tag (FR-9.36) remain mandatory and outside the frame; `docs/styling-your-page.md` and `policy/REVIEW.md` restate that a page must never imitate Mumcord's own dialogs for passwords, certificates, server connections or permission prompts (existing review rule).
- **Privacy**: tokens are theme values only; they carry no user data. The theme of the user is visible to the plugin page, as it already is to the plugin core (FR-6.23).
- **Performance**: theme change reaches every open 1.2 page within 200 ms (FR-13.15); `kit.css` ≤ 30 KiB uncompressed; fonts are the app's existing font files; serving the kit adds no measurable page-load time against a local package.
- **Compatibility**: 1.0 and 1.1 plugins unchanged (FR-13.8, FR-13.23); registry and older clients as in spec 09.
- **Platforms**: Windows 10+ (WebView2), macOS 12+ (WKWebView, child web view), Linux (WebKitGTK 4.1), as today.

## 10. Build, packaging & release

### 10.1 Release

- Mumcord 0.8.0 ships WIT snapshots 1.0.0, 1.1.0 and 1.2.0, SDK feature `1.2`, the kit files and the docs of FR-13.31.
- `Mumcord/plugins` registry CI accepts `api` 1.2 (FR-13.6) before the first 1.2 plugin version is published; docs and template (FR-13.32, FR-13.33) land with or right after Mumcord 0.8.0.
- No 1.2 version of any plugin is published to the official registry before Mumcord 0.8.0 is released.

### 10.2 Test plugins and fixtures (Mumcord e2e; platform only)

| Fixture | Purpose |
|---|---|
| `test-kit` (1.2, stage + side panel + dialog) | `kit.html`: one of every kit element in every state of FR-13.24, inside `.mc-kit`; `plain.html`: same HTML without linking the kit; `probe.html`: reports the `--mc-` tokens, the attributes and every `mumcord-theme-change` event back over the bridge, and the values its first script (a file of the package) reads before the page renders |
| `test-kit-reserved` (1.2) | Package with `web/_mumcord/v1/kit.css` and `web/_MUMCORD/x.css` → refused at install |
| `test-surface` (1.1, existing) | Gets no tokens, no event; `_mumcord/v1/kit.css` answers "not found" |
| Reference page (debug builds only, inert in release) | An app-side page showing Mumcord's own button, radio, checkbox, switch, Select trigger, text field and dialog in the same states, for AC-13.9 |

Setup: local mumble-server ≥ 1.4, one Mumcord client in Lobby, the local fixture registry (`mumcord-plugin serve-registry`) with a third-party repository holding the fixtures; for AC-13.12 a fixture `v1.1` index with a 1.2 version and a Mumcord 0.7.x build. Never a public server.

## 11. Acceptance criteria (verifiable in the running client)

Setup for all unless stated otherwise: section 10.2.

**Versioning**
1. **AC-13.1** `mumcord-plugin new` creates a plugin with `api: "^1.2"`; it builds with SDK feature `1.2`, `mumcord-plugin check` passes, and its component imports `mumcord:plugin@1.2.0`. The snapshot test fails if `wit-1.2.0.snapshot` changes.
2. **AC-13.2** The 1.0 and 1.1 test plugins of spec 09 (`test-api10`, `test-surface`, `test-stage`) still load and pass their existing E2E unchanged on 0.8.0.

**Tokens**
3. **AC-13.3** `test-kit` `probe.html` in the stage: the page's first script reads every token of FR-13.9 as non-empty and `data-mc-mode`, `data-mc-scheme`, `data-mc-reduced-motion` with the current values; colour tokens equal the app's current values for dark, light and a custom theme.
4. **AC-13.4** With `probe.html` open in stage, side panel and dialog at once, switching dark → light → a custom theme → dark updates every token and attribute in all three within 200 ms, fires exactly one `mumcord-theme-change` per change in each frame with matching `detail`, and the page keeps its state (a counter it holds keeps its value; no reload).
5. **AC-13.5** Turning visual effects off sets `data-mc-reduced-motion="true"` and both duration tokens to `0ms`, live.
6. **AC-13.6** Under OS high contrast (manual, Windows; emulated forced colours in E2E where the web view supports it) `data-mc-mode` is `high-contrast` and every kit control on `kit.html` keeps a visible border and focus ring.
7. **AC-13.7** `test-surface` (1.1) has none of the `--mc-` tokens or attributes, gets no theme event on a theme change, and `_mumcord/v1/kit.css` returns "not found".

**Kit**
8. **AC-13.8** `test-kit` `plain.html` (does not link the kit) has the same computed styles on all elements as before 0.8.0 and as in a 1.1 plugin; linking the kit without `.mc-kit` and without `mc-` classes leaves the computed styles of all native controls unchanged.
9. **AC-13.9** For each control of FR-13.24 and each state, the computed styles that make the look (border colour and width, background, text colour, radius, height, padding, font family, size and weight, focus outline) of the kit control on `kit.html` equal those of the app's own control on the reference page, in dark, light and a custom theme.
10. **AC-13.10** `kit.html` uses Instrument Sans for text and Bricolage Grotesque for dialog titles and section labels (`document.fonts` reports both loaded from `_mumcord/v1/fonts/`); with the network blocked the page looks the same, and no request leaves the machine.
11. **AC-13.11** Keyboard only: Tab reaches every kit control on `kit.html` in order with a visible focus ring; Space toggles checkbox and switch, arrow keys move between radio buttons, the dropdown opens and selects with the keyboard; a screen reader (manual, NVDA and VoiceOver) announces the switch as a switch with its state.

**Registry and reserved path**
12. **AC-13.12** A fixture `v1.1` index lists a plugin with versions 1.0.0 (`api` 1.1) and 1.1.0 (`api` 1.2, no `min_mumcord`): Mumcord 0.8.0 installs 1.1.0; Mumcord 0.7.x installs 1.0.0 and its Updates tab shows "Update available after updating Mumcord"; installing the 1.1.0 package file directly on 0.7.x is refused before anything is written. Registry CI rejects that version with `min_mumcord: "0.7"` and never writes it into `v1/index.json`.
13. **AC-13.13** Installing `test-kit-reserved` is refused with "uses the reserved folder `web/_mumcord`" and nothing is written to disk; `mumcord-plugin build` and `check` refuse it too; a `show` call with entry `_mumcord/v1/kit.css` or `_MUMCORD/x.html` returns the existing invalid-entry error.
14. **AC-13.14** The kit responses carry the same `Content-Security-Policy` and `Permissions-Policy` headers as a `web/` file of the same plugin and the MIME types of FR-13.23.

**Release and docs (manual)**
15. **AC-13.15** Screenshots of `test-kit` `kit.html` next to Mumcord's Settings in dark, light and high contrast are in the pull request and committed to `docs/images/screenshots/`; the product owner confirms they look like Mumcord's own controls.
16. **AC-13.16** `docs/plugins/api-1-2.md`, the plugins-repo pages of FR-13.32 and the template of FR-13.33 exist; the example page from "Styling your page", copied into a fresh `mumcord-plugin new` plugin, renders like `kit.html`.

## 12. Decision log

| Question | Decision | Note |
|---|---|---|
| API version for a pure-web feature | Plugin API 1.2, additive; SDK feature `1.2`; needs Mumcord 0.8.0 | Product owner, 2026-11-08 |
| WIT change | A `mumcord:plugin@1.2.0` package identical to 1.1.0 except the version | Keeps "`api` matches WIT imports" (FR-9.20/21) with no special case; no new function needed |
| Registry | 1.2 versions only in the `v1.1` index, `min_mumcord` default and minimum 0.8; no new index | Product owner; spec 09 rules unchanged |
| Token prefix and names | `--mc-` prefix; FR-6.23 names after `--mc-color-`; extra tokens named after Mumcord's own variables | Product owner (prefix, FR-6.23 names); extra list chosen so the kit needs nothing else |
| High contrast | `data-mc-mode="high-contrast"` while OS forced colours are active; `data-mc-scheme` keeps the base dark/light | Mumcord has no high-contrast theme of its own; it follows the OS |
| Accent and scaling | Accent is a colour token (custom themes included); no scale token, `rem` sizing follows system scaling and web view zoom | Mumcord has no UI scale or font-size setting |
| Who gets tokens | Only 1.2+ plugins | 1.0/1.1 pages stay byte for byte as in 0.7.x (FR-9.8 spirit) |
| Event | `mumcord-theme-change` on `window`, not on first load | Pages using CSS variables need no code |
| Kit location | `_mumcord/v1/` inside the plugin's own address | No CSP change; `v1` = API major |
| Kit form | CSS only, no script; native controls restyled; open dropdown list stays the system list | "Plain HTML, no framework" (#691); no JS keeps the attack surface flat |
| Kit scope | `mc-` classes anywhere; native controls only inside `.mc-kit`; `:where()` zero specificity | Never leaks onto pages or parts of pages that don't opt in |
| Reserved folder in packages | Refused for 1.2 packages; for 1.0/1.1 only a `check` warning, files never served | No 1.0/1.1 package is refused retroactively |
| Whiteboard | Moves to the kit as version 1.1.0 (`api ^1.2`) in a separate `Mumcord/plugins` pull request, merged only after Mumcord 0.8.0 ships | Product owner |

## 13. Open points & risks

- **Risk: native control styling differs per web view.** WebKitGTK, WKWebView and WebView2 differ in how far `appearance: none` reaches (mostly `select` and `input[type=number]`). Mitigation: AC-13.9 runs on all three platforms in CI where available and manually on macOS; where a platform can't match a detail, the kit falls back to the native look for that detail and the docs say so.
- **Risk: computed-style equality is brittle.** Mitigation: AC-13.9 compares only the listed look properties, not layout; the reference page and the kit share the same tokens.
- **Risk: `<dialog>` in a sandboxed frame.** `showModal()` behaviour without `allow-modals` may differ per web view. The kit styles `.mc-dialog` on any element; the docs example uses a `<div role="dialog">` so it works everywhere.

## 14. Impact on the roadmap

- **Roadmap**: `specs/roadmap.md` lists this spec under the 0.8.0 milestone.
- **Spec 06 / 09**: FR-6.23, FR-6.47 and spec 09 §4.6 get a pointer to this spec; their rules stay valid unchanged for 1.0 and 1.1 plugins.
- **Official plugins (`Mumcord/plugins`)**: Whiteboard 1.1.0 (`api ^1.2`) replaces its own control styles (Export dialog radio buttons, dropdowns, switch, buttons) with the kit and its colours with `--mc-` tokens, in a separate pull request merged only after Mumcord 0.8.0 is released; its E2E stays in the plugins repo and runs against Mumcord 0.8.0. Users on 0.7.x keep Whiteboard 1.0.x (FR-13.7). Other official plugins with web pages may follow later.

## 15. Implementation split

One fresh agent per task.

| Order | mumcord | plugins |
|---|---|---|
| Step 1 | T1 WIT 1.2.0 package, snapshot, host linking, `SUPPORTED` 1.2, SDK feature `1.2`, `mumcord-plugin new`/`build` default (FR-13.1 … 13.3); T2 reserved path in protocol, `parse_entry`, package validation, CLI checks (FR-13.19, 13.23, 13.28, 13.29) | R1 registry CI accepts `api` 1.2 with `min_mumcord` ≥ 0.8, only in `v1.1` (FR-13.6) |
| Step 2 | T3 tokens: mapping table, theme state in Rust, initial values in served HTML, bootstrap apply + live updates + event, both frame transports (FR-13.8 … 13.17); T4 kit: `ui/src/plugin-kit/kit.css`, fonts, bundling, serving (FR-13.18 … 13.27) | — |
| Step 3 | F1 `test-kit`, `test-kit-reserved`, reference page; E1 E2E AC-13.1 … 13.14; screenshots AC-13.15 | D2 `docs/api.md`, "Styling your page", publishing docs, `template/` (FR-13.32, 13.33) |
| Step 4 | D1 `docs/plugins/api-1-2.md`, registry-format doc, spec pointers, CHANGELOG (FR-13.31, 13.34) | After Mumcord 0.8.0 ships: W1 Whiteboard 1.1.0 on the kit (section 14) |
