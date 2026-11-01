# Epic 3 acceptance walkthrough — issue #195

Walked in the running client (Tauri debug build, Xvfb, local test servers `v15` + `v15b`, GNOME
keyring for the Secret Service), 2026-10-31. Screenshots below are from that run and live in
`docs/images/screenshots/`.

## What is verified where

| Area                                     | How                                                                 | Result                                                                    |
| ---------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| AC-3.2 – AC-3.14 identity manager        | `e2e/specs/servers/identities.e2e.ts` (14 tests)                    | all green                                                                 |
| AC-3.20 – AC-3.29 add/edit server dialog | `e2e/specs/servers/server-dialog.e2e.ts` (9 tests)                  | all green                                                                 |
| AC-3.1 upgrade from a 0.4.x profile      | manual only: needs an old profile, so it cannot be run in the suite | store migration tests in `crates/store/src/identities.rs` cover the rules |

Run:

```bash
npm run e2e -- --spec e2e/specs/servers/identities.e2e.ts --spec e2e/specs/servers/server-dialog.e2e.ts
```

Both files were added to `e2e/shards.json` (the servers shard).

## Screenshots

Identity manager (dark, then the same screen at 200 % text):

![Identity manager, dark](../images/screenshots/settings-identity-walkthrough-dark.png)
![Identity card at 200 % text, dark](../images/screenshots/settings-identity-walkthrough-200-dark.png)
![Identity card at 200 % text, light](../images/screenshots/settings-identity-walkthrough-200-light.png)

Identity details:

![Identity details, dark](../images/screenshots/settings-identity-details-walkthrough-dark.png)

Add a server dialog:

![Add a server, Connection tab, dark](../images/screenshots/server-dialog-connection-walkthrough-dark.png)
![Add a server, Identity tab, dark](../images/screenshots/server-dialog-identity-walkthrough-dark.png)
![Add a server, Connection tab, light](../images/screenshots/server-dialog-connection-walkthrough-light.png)
![Add a server, Behaviour tab, light](../images/screenshots/server-dialog-behaviour-walkthrough-light.png)

## Findings

1. **Accessibility, fixed.** The identity card's "Set as default" button had an accessible name
   (`Set {{identity}} as the default identity`) that did not contain its visible text, which axe
   reports as `label-content-name-mismatch` (serious, WCAG 2.2 AA). The name is now the visible
   words plus the identity, in every locale.
2. **Layout, fixed.** At 200 % text the card's action row starved the text column: the label and
   the name line were cut off. The card now stacks the actions on their own row when there is no
   room (container query on the identity list), and the spec asserts the card's own text elements
   are not clipped.
3. **Error kind, added.** AC-3.3 says a second import of the same certificate is refused. The
   engine reported the generic `invalidInput`, so the UI could not name it as a duplicate. Added
   `duplicateIdentity` to the engine, the Tauri bindings and the settings error mapping.
4. **Reconnect, fixed.** Changing the identity of a connected server only showed the server
   instead of reconnecting it (AC-3.10), because the UI store still thought the connection was
   open when `requestConnect` ran. The store now connects directly after the disconnect.
5. **AC-3.24 cannot be met as written.** The "Test connection" ping is one round trip, while the
   live connection shows a smoothed figure; on a local server the two are not in the same range,
   and a server that was only tested (never connected) has no live figures at all — the stats are
   `null`. The spec therefore checks the engine's own bound for the probe and leaves the comparison
   of live figures to the connection-info spec (AC-1.23).
6. **Side effect of the test connection.** The probe connects with the client's own certificate, so
   a standard murmur server kicks the live session ("You connected to the server from another
   device"). The dialog promises that testing does not connect for real; on murmur it does, and the
   live connection is dropped. Worth deciding for Epic 3 follow-up.
7. **Test tooling notes.** Native GTK file choosers cannot be driven through WebDriver, so the
   import/export specs type the path with `xdotool`; server context menus are opened with a
   synthetic `contextmenu` event because a WebDriver right-click leaves the page unable to receive
   typed text.

## Flakiness

The identity spec is stable over three consecutive runs (14 + 9 tests). The header badge check in
AC-3.10 read the badge before the reconnect finished and failed once; it now waits for the badge to
name the identity.
