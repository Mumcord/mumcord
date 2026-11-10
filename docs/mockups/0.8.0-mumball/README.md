# Mumball mockups (spec 14)

Screen mockups for the community pinball plugin Mumball, described in `specs/14-pinball-plugin-specification.md`. Every screen exists in dark and light.

| File | Screen |
|---|---|
| `01-lobby-*.png` | Lobby (starter's view) |
| `02-play-*.png` | Play screen, layout A |
| `03-paused-*.png` | Own table paused |
| `04-out-*.png` | You're out, with the 60-second countdown |
| `05-results-starter-*.png` | Results, starter's view |
| `06-results-*.png` | Results, other players' view |
| `07-chat-*.png` | Results card in the chat |
| `08-popout-*.png` | Pop-out window, layout B |

The PNGs are rendered from `mumball.html` (`?screen=<name>&theme=<dark|light>`) at 1280 × 800 (pop-out 540 × 860) with a device scale of 2. The page loads Mumcord's fonts from `node_modules`, so run `npm install` in the repository root before rendering.

The table is an original "On Air" radio-studio design. Mumcord's window, sidebar, chat and activity header are only drawn for context; the plugin owns the game frame below the activity header.
