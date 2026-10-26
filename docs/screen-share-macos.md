# Screen sharing on macOS: manual test checklist

macOS capture (task SS12) is built and checked on Linux by cross-compiling only (clippy for
`aarch64-apple-darwin`); nothing of it has run on a Mac yet. This checklist is the first real
test. Run it against a local test server, never a public one.

## How it works

- **Picture:** ScreenCaptureKit. On macOS 14 and newer the system picker
  (`SCContentSharingPicker`) lets the user choose a window, an app or a whole screen.
- **macOS 12.3 and 13** have ScreenCaptureKit but no system picker. The spec asks for the
  system picker and no picker of our own, so there the main screen is shared whole (like X11 on
  Linux). Before 12.3 sharing fails with "not supported". Both are below the supported range
  (current and previous macOS major), so this is a fallback, not a target.
- **Sound:** captured in the same ScreenCaptureKit stream, limited to the shared content, and
  always without Mumcord's own output (`excludesCurrentProcessAudio`), so viewers never hear the
  voice chat back. A window or an app shares that app's sound; a whole screen shares the sound
  of every app except Mumcord. Needs macOS 13 or newer.
- **Size:** the system scales the picture into 1280×720 (the encoder's largest step) before
  Mumcord sees it, so a Retina or 5K screen costs no more than a 720p one.
- **Permission:** screen recording needs the user's consent. On macOS 14+ choosing in the
  system picker is that consent. On 12.3 and 13 Mumcord asks for the "Screen & System Audio
  Recording" permission; without it the share fails and the sharer sees "Allow it in System
  Settings … then restart Mumcord" (macOS applies the permission only after a restart).
- **Stop from the system:** the menu bar's screen-sharing indicator can stop the share; the
  share then ends in Mumcord too, as when the shared window closes.
- **Bundle config:** no Info.plist key or entitlement is needed. Screen recording has no usage
  description key, and the hardened runtime has no entitlement for ScreenCaptureKit.

## Setup

- A Mac (macOS 14 or newer; a macOS 13 machine for the fallback steps if available) and a
  second machine with Mumcord, both in the same channel on the local test server, direct
  connections allowed on both.
- On the Mac: an app that plays sound (a browser with a video, Music) and a second app that also
  plays sound.

## Checklist (macOS 14+)

1. **Picker opens:** click "Share screen". The system picker appears and offers windows, apps
   and screens.
2. **Cancel:** close the picker without choosing. No share starts, no error message.
3. **Window share:** pick the browser window playing a video. The viewer sees the window at up
   to 720p30 and the menu bar shows the sharing indicator.
4. **Only that app's sound:** the viewer hears the browser's sound, not the second app's sound
   and not the voice chat (talk on the second machine: the sharer hears it, the viewer does not
   hear it twice).
5. **Sound switch:** turn the share's sound off and on in the sharer controls; the viewer's
   sound stops and comes back.
6. **App share:** stop, share again and pick "app" mode with the second app. Its windows are
   shown and only its sound is heard.
7. **Whole screen:** share a screen. The viewer sees the whole screen and hears every app's sound
   except Mumcord's.
8. **Retina picture:** on a Retina or external 4K screen the picture is sharp and the sharer's
   CPU use stays similar to a 1080p screen.
9. **Window closed:** share a window and close it. The share ends by itself in Mumcord within
   about a second, and the viewer's player closes.
10. **Menu bar stop:** share again and stop it from the menu bar's sharing indicator. The share
    ends in Mumcord too.
11. **Pause:** pause and resume the share; the picture freezes and continues, no sound while
    paused.
12. **Quality drop:** with the upload throttled the picture steps down to 480p15 as on Linux.

## Checklist (macOS 13, if available)

1. Remove Mumcord from System Settings → Privacy & Security → Screen & System Audio Recording.
2. Click "Share screen". macOS asks for the permission and Mumcord shows "Mumcord may not record
   your screen. Allow it in System Settings …". No share starts.
3. Allow it in System Settings and restart Mumcord. "Share screen" now starts at once without a
   picker and shares the main screen with the sound of every app except Mumcord.

## What to report

- macOS version and Mac model (Apple silicon or Intel).
- For each step: works / does not work, and what the viewer saw and heard.
- On a failure: the Mumcord log (Settings → Diagnostics); it never contains pictures or sound.
