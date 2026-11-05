# Plugin security score

Mumcord shows a security score for every **unofficial** plugin (plugins from third-party repositories and your own developer plugins). It helps you decide whether to install a plugin. This page explains exactly how the number is worked out, so users can check it and plugin authors can plan for it.

Official plugins, reviewed by the Mumcord maintainers, show "Official · reviewed" and no number.

## What the score is, and is not

The score is a number from 0 to 100 and a band. **Higher means more risk.**

| Band        | Score     |
| ----------- | --------- |
| Low risk    | 0 to 29   |
| Medium risk | 30 to 59  |
| High risk   | 60 to 100 |

Colour, icon, word and number are always shown together, so the band never depends on colour alone.

The score is an **estimate worked out on your computer** from what a plugin asks for and who published it. It is not a guarantee. It does not read the plugin's code, scan for malware or see what a website sends back. Nothing leaves your computer to calculate it: there is no remote service and no telemetry. The same inputs always give the same score.

Every score shows its **formula version** ("Formula version 1"). The weights live in [`crates/plugins/score-weights.toml`](../crates/plugins/score-weights.toml) and ship with the app. A change to any weight bumps the version, updates this page and is listed in the changelog.

## The formula (version 1)

```
Score = min(100, round(E × F) + H)
```

and then, if the plugin holds `net.http` or `net.origins` and the result is below 30, the score is raised to 30 (the floor).

- **E** is the capability score: what the plugin may do.
- **F** is the trust factor: who published it and how open they are.
- **H** is the recent-changes score: what the plugin recently started asking for.

### E: capability points (cap 100)

E counts every permission the manifest may ask for, **optional ones included**, because you can grant them later.

| What                                                                                                                         | Points                                 |
| ---------------------------------------------------------------------------------------------------------------------------- | -------------------------------------- |
| `net.http`, `net.origins`, `secrets` (high risk)                                                                             | +20 each                               |
| Web surfaces: `ui.stage`, `ui.side-panel`, `ui.webview`                                                                      | +10 for the first, +4 for each further |
| `peers.relay`                                                                                                                | +8                                     |
| `events.voice`, `events.presence`, `events.chat-activity`, `peers.messages`, `chat.send`, `chat.post-image`                  | +6 each                                |
| Every other permission (green, such as `ui.panel`, `ui.chat-card`, `ui.app-menu`, `audio.effects`, `files.save`, `commands`) | +1 each, at most +6 in total           |
| Declared websites, from the second one on                                                                                    | +6 each, at most +18                   |
| Package size: up to 2 MiB / up to 10 MiB / up to 30 MiB / larger                                                             | 0 / +2 / +5 / +8                       |
| WASM modules: 1 / 2 to 3 / 4 or more                                                                                         | 0 / +2 / +4                            |

Permissions that are not in any list use the risk shown in the install prompt: red counts as high, yellow as medium, green as low.

### F: trust factor (starts at 1.00, never below 0.75 or above 1.30)

F is a product of percentage effects, added together and clamped.

| Signal                                                                                             | Effect              |
| -------------------------------------------------------------------------------------------------- | ------------------- |
| A source link (https) is given / missing                                                           | -5 % / +10 %        |
| Licence is OSI-approved (bundled SPDX list) / proprietary / not declared                           | -5 % / +5 % / +10 % |
| The repository says the build is reproducible ("claimed by the repository")                        | -5 %                |
| The author key has been known to this device for 180 days or more / 30 to 179 days / under 30 days | -10 % / 0 / +10 %   |
| You entered an "Expected key" when adding the repository and it matched                            | -5 %                |

Key age counts from the day **this device first saw the key** in a refreshed index, never from a date the repository states. Developer plugins (your own, loaded in Developer mode) have F fixed at 1.00.

### H: recent changes (cap 25)

H looks at the last 5 published versions (the one being scored included) within the last 180 days. For every permission that was added in that window and that the oldest of those versions did not have: **+4**, or **+10** for a high-risk permission or a web surface. This catches plugins that stay quiet for a while and then ask for more. History comes from the index and from versions stored on this device. Without history H is 0 and the breakdown says "no history available".

### Worked example

A small plugin with `ui.panel`, `commands` and `chat.send`, one WASM module, 100 KiB, an https source link, an MIT licence and an author key known for a year:

- E = 1 + 1 + 6 = 8
- F = 1.00 - 5 % - 5 % - 10 % = 0.80
- H = 0
- Score = round(8 × 0.80) + 0 = 6, **Low risk**

The same plugin with no source link, no licence and a brand-new key has F = 1.30 (10 + 10 + 10 = +30 %) and scores 10, still Low. Add `net.origins` with two websites and the floor of 30 applies: Medium at the least.

The "Why this score" table on every plugin's details page shows each line of your plugin's real calculation with the formula line "Score = E points × F (±%) + H = score".

## Hard stops

Some things are refused, not scored. The install prompt says why and shows no score:

- an invalid signature, a hash or manifest mismatch;
- a plugin the registry has revoked;
- a declared website that is not allowed: not `https` or `wss`, a bare IP address, a wildcard, user info, a private, local or `localhost` name, a path, or more than 8 websites.

Developer mode may additionally use `http://localhost:<port>`; a packaged plugin with such an origin is a hard stop.

## Where the score appears

Browse cards, the details page, the install prompt, the update prompt and the Updates tab (before and after), and Settings → Plugins → Installed (also for developer plugins, which are scored for information only: nothing is blocked).

Browse scores from the index. The install prompt scores the verified package. If the package scores higher than its listing, the prompt says so and uses the package score.

## What the score changes

- **One confirmation box.** Install needs a tick when the plugin is High risk, or asks for `ui.webview` or `net.origins` while unofficial, or comes from a repository you just added.
- **"Don't allow high-risk plugins"** (Settings → Plugins → Installed, off by default): High plugins cannot be installed and updates that would make an installed plugin High are held back. Installed plugins keep running; official and developer plugins are never affected.
- **Automatic updates** install only when the update asks for no new permissions, no new websites and no new author key, and the score rises by less than 10 points and stays in the same band or goes down. Anything else waits in the Updates tab with the reason.
- **Update review.** An update that adds websites lists them all, marks the new ones, and warns when a new website is not on the plugin's own domain. An update that makes the plugin High needs its own tick.

## Limits: what the score cannot tell you

Read the number as a hint about how much a plugin could do, never as proof that it is safe or unsafe.

- **Claims are claims.** The source link, the licence and the reproducible-build marker come from the repository. Together they move the score by about 15 %. Mumcord does not check that the linked source is what the plugin runs.
- **Splitting a plugin.** An author can spread powers over several plugins, each with a modest score. The score judges one plugin at a time.
- **Harmless-looking abuse.** A plugin with few permissions can still annoy people or mislead them. Chat, names and presence are enough for a convincing scam.
- **Websites are black boxes.** Mumcord limits which websites a plugin may contact and can show what you approved, but it cannot see what a website sends back or does with what it receives. Websites can see your IP address and anything the plugin sends them.
- **Waiting out key age.** A key becomes "known" simply by existing on your device for 180 days. A patient author can wait.
- **It is not a review.** Only plugins marked "Official · reviewed" were looked at by a person.

If a plugin's score looks wrong to you, the "Why this score" table shows every input. Please report scoring problems at <https://github.com/Mumcord/mumcord/issues>.
