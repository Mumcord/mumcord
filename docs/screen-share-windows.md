# Screen sharing on Windows: manual test checklist

Windows capture (task SS11) is built and checked on Linux by cross-compiling only (clippy for
`x86_64-pc-windows-msvc`); nothing of it has run on Windows yet. This checklist is the first
real test. Run it against a local test server, never a public one.

## How it works

- **Picture:** the Windows system picker (Windows.Graphics.Capture) lets the user choose a
  window or a whole screen. Needs Windows 10 version 1903 or newer.
- **Sound:** only the shared app's own sound, through WASAPI process loopback (the app's process
  and its child processes). Needs Windows 10 version 2004 (build 19041) or newer.
- **No sound** is sent when a whole screen is picked, when the picked window's title belongs to
  more than one app, when Mumcord's own window is picked (its sound is the voice chat), or on
  Windows older than 2004.
- Which app is behind a picked window is found by its title, because the picker only returns
  the title. An app that renames its window while the picker is open can end up without sound.

## Setup

- Two machines (or one Windows machine and a Linux one) with Mumcord, both in the same channel
  on the local test server, direct connections allowed on both.
- On Windows: an app that plays sound (a browser with a video, a music player) and a second app
  that also plays sound.

## Checklist

1. **Picker opens:** click "Share screen" on Windows. The system picker appears over Mumcord's
   window and lists windows and screens.
2. **Cancel:** close the picker without choosing. No share starts, no error message.
3. **Window share:** pick the browser window. The viewer sees it within a few seconds, the yellow
   capture border appears around the window on the sharer's side.
4. **Moving picture:** play a video in the shared window; the viewer sees smooth motion at
   about 30 fps (up to 1080p).
5. **Still picture:** pause the video and leave the window untouched. A viewer who starts
   watching now still gets the picture (not a black or empty view).
6. **Resize:** make the shared window larger and smaller. The viewer's picture follows without
   distortion or garbage at the edges.
7. **App sound only:** with sound on, the viewer hears the browser video. Play music in the
   second app at the same time: the viewer must not hear it.
8. **No voice echo:** talk in the voice channel while sharing; the viewer must not hear the
   voice chat a second time through the share.
9. **Browser child processes:** share a Chrome or Edge window playing a video; its sound comes
   through (it plays from a child process of the browser).
10. **Sound off and on:** turn the share's sound off and on again in the sharer controls; the
    viewer's sound stops and comes back.
11. **Whole screen:** stop, start again and pick a whole screen. The picture works; no sound is
    sent and the sharer sees the no-sound hint.
12. **Mumcord's own window:** share Mumcord's window. The picture works, no sound is sent.
13. **Window closed:** close the shared app's window. The share stops by itself within about a
    second and the viewers' view closes.
14. **Stop:** start a share and stop it with Mumcord's stop button. The yellow border goes away.
15. **No GPU:** in a virtual machine without GPU acceleration, sharing still works (software
    Direct3D); expect higher CPU use.
16. **Old Windows (if available):** on Windows 10 1909 or older, sharing a window works without
    sound and the sharer sees the hint; on 1809 or older, sharing is refused with a clear
    message.

Write down the Windows version (`winver`) with the results. Failures become bug tasks in the
Epic 2 tasks file.
