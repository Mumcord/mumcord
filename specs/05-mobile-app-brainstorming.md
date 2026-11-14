# Epic 5 – Android and iOS app · Brainstorming

> Basis: `specs/roadmap.md` → Epic 5 "Android and iOS"; overview issue #228.
> This is a fresh start. The earlier child issues #231–#245 and the earlier mockups (wiki `images/mockups/epic4/`) were made without a specification; they are treated as input only, not as decisions.
> Process: We work in several rounds. Each round contains questions; you answer directly in this file.
> Once all questions are answered, `specs/05-mobile-app-specification.md` is created from it.
> New mockups are produced together with round 2 (UI & UX), once the scope from round 1 is known, so they show the screens that will really be built.
> Already settled (not asked again):
> - Pure client against unmodified standard Mumble servers; nothing may be needed on the server side → CLAUDE.md, Spec 01.
> - Tauri 2 was chosen in Spec 01 partly because it offers a path to mobile (Q1.7). Whether that path is really taken is asked in round 4, with a recommendation.
> - Opus only, automatic bitrate, noise suppression, echo cancellation and automatic gain on by default → Spec 01 (FR-1.23, FR-1.24).
> - Identities are certificates kept only in the secure storage of the operating system; import and export as a password-protected `.p12` file exist on desktop → Spec 03 identity manager (FR-3.6, FR-3.8, FR-3.18).
> - Theme model (7 key colours, dark and light, WCAG AA contrast), fonts and the 24 locales → Spec 01, Spec 02.
> - Chat, private messages, mentions, blocking, private message rules and Do Not Disturb rules as on desktop → Spec 01, Spec 02, Spec 03. Only what differs on a phone is asked here.
> - Distribution for now: signed APK on GitHub releases and TestFlight; no app stores → issue #228. Store releases are roadmap Epic 6.

## How to answer

- **Single choice** (`◉`): Exactly **one** box has an `[x]`. To change your choice, just move the `x`.
- **Multiple choice** (`☑`): Tick as many boxes as you like.
- ⭐ marks my **recommendation**. It is already preselected. If you agree, you don't need to change anything.
- Below each question there is `✍️ Note:`. Add anything there, e.g. questions back, edge cases or "not sure, explain this to me".
- I explain technical terms briefly. For purely technical questions (audio, network, build and signing) it's perfectly fine to follow my recommendation.

## Planned rounds

1. Functionality & scope: what the first mobile version does and what stays on desktop
2. UI & UX: navigation, the screens, touch behaviour (ASCII wireframes plus new mockups in dark and light)
3. Voice on a phone: background and lock screen, phone calls, headsets and speaker, battery and network changes, notifications without a push server
4. Technology & delivery: how the app is built, moving an identity to the phone, signing, APK and TestFlight, licence questions, testing on emulators and devices
5. Review & remaining open points

(The plan may change depending on the answers.)

---

## Round 1 – Functionality & scope

Where we start from, in plain words:

- The desktop app is built from two parts: the "engine" (connection, voice, chat, storage, written in Rust) and the user interface (a web-style interface inside the app window).
- The engine parts for connection, voice codec, encryption and storage are a good base for a phone. They need adapting, not rewriting.
- Several desktop features are tied to desktop operating systems and cannot simply be switched on for a phone: screen capture, camera capture with background blur, the in-game overlay, positional audio for games, plugins, global hotkeys, the tray icon, pop-out windows and the desktop updater.
- The current interface is made for a mouse and a wide window (hover menus, right-click menus, drag and drop). A phone needs its own layout for touch. It can reuse the look (colours, fonts, icons), the texts in all 24 languages and much of the logic.

So this epic is a real porting project, not a recompile. A tight first version matters. The questions below decide how tight.

### A · Goal and platforms

#### Q1.1 What is the first mobile version mainly for? `◉`
This sets the bar for everything else.

- [x] ⭐ **A companion for people who already use a Mumble server: join, talk, listen, chat.** Everything needed for a normal voice session on the go; setup-heavy and admin tasks stay on desktop for now.
- [ ] A full replacement for the desktop app. Much more work; many desktop features have no phone equivalent yet.
- [ ] Listen and chat only, no talking. Smallest version, but misses the main point of a voice app.

✍️ Note:

#### Q1.2 Android and iOS: together or one after the other? `◉`
Both apps come from the same code. iOS has stricter rules for background audio and needs an Apple developer account and a Mac build machine; Android is easier to build, install and test.

- [x] ⭐ **Both in this epic, Android leads.** Every feature is first built and verified on Android, then brought to iOS. One spec, one code base, the iOS-specific risks do not block Android testers.
- [ ] Both strictly in parallel; nothing counts as done until it works on both. Slower start, no platform is ever behind.
- [ ] Android only in this epic, iOS as its own later epic.

✍️ Note:

#### Q1.3 Phones only, or tablets too? `◉`

- [x] ⭐ **Phones. Tablets run the same app with the phone layout, slightly widened.** No separate tablet design in the first version.
- [ ] Phones and a real tablet layout (two columns, like desktop). Roughly doubles the design and testing work for the screens.
- [ ] Phones only; tablets are not supported at all.

✍️ Note:

---

### B · Core features of the first version

#### Q1.4 Which of these belong in the first mobile version? `☑`

- [x] ⭐ Server list with favourites: add, edit, remove, connect
- [x] ⭐ Connecting with server password, user name, and the certificate warnings (unknown or changed server certificate) as on desktop
- [x] ⭐ Channel tree with the people in each channel; tap to join or switch channel; channel passwords
- [x] ⭐ Talking with voice activity (the microphone opens when you speak)
- [x] ⭐ Talking with push-to-talk as a large on-screen button
- [x] ⭐ Mute yourself, deafen yourself
- [x] ⭐ Per person: change their volume, mute them for yourself
- [x] ⭐ Channel text chat: read and write, including links and images shown inline
- [x] ⭐ Private messages
- [x] ⭐ Local chat history on the phone (as on desktop, kept on the device only)
- [x] ⭐ Mentions (`@name`) with highlighting
- [x] ⭐ Identities: create a new one on the phone, or import one from desktop
- [x] ⭐ Dark and light design following the phone setting, in all 24 languages
- [ ] Editing your own comment and avatar from the phone (later)
- [ ] Custom themes and per-server themes (later; the first version ships the standard dark and light theme)
- [ ] GIF picker and emoji reactions in chat (later)
- [ ] Opening `mumble://` links from other apps to connect directly (later)

✍️ Note:

#### Q1.5 Several servers connected at the same time? `◉`
On desktop you can be connected to several servers at once: voice on one, reading along on the others (Spec 02). Each open connection costs battery and mobile data.

- [x] ⭐ **One server at a time on the phone.** Switching servers disconnects the current one. Simple, battery-friendly, clear on a small screen.
- [ ] As on desktop: several servers at once. More battery use and a more complex layout.
- [ ] One at a time in the first version, several later (same as the first option, but written down as a planned follow-up).

✍️ Note:

#### Q1.6 Which talk mode is the default on a phone? `◉`
On desktop the default is voice activity. On a phone people are often outside or in noisy places.

- [x] ⭐ **Voice activity, with noise suppression on.** Hands-free, works with the phone in the pocket and a headset. Push-to-talk is one tap away.
- [ ] Push-to-talk. Safest against background noise, but you must hold the phone and look at it.
- [ ] Ask on first start (a short choice screen with both explained).

✍️ Note:

---

### C · Desktop features on the phone

#### Q1.7 Which of the larger desktop features come to the first mobile version? `☑`
Each ticked box makes the first version noticeably later. "View only" means you can watch what others share but not share yourself.

- [x] ⭐ None of these in the first version. All of them stay desktop-only for now and are listed in the spec as later candidates.
- [ ] Watching a screen share or camera of someone else (view only)
- [ ] Sharing your own phone camera
- [ ] Sharing your phone screen
- [ ] File sharing (sending and receiving files through the optional relay)
- [ ] Watch together
- [ ] Whisper and shout (talking to selected people or channels only)
- [ ] Admin and moderation basics (kick, ban, move people, server mute)
- [ ] Full admin tools (permissions editor, ban list, registered users, channel management)
- [ ] Activity log
- [ ] Plugins

Not offered at all, because they only make sense on a desktop: in-game overlay, positional audio for games, packet inspector, global hotkeys, tray icon, pop-out windows.

✍️ Note:

#### Q1.8 If you could pick one "next" feature after the first version, which is it? `◉`
This does not add work now. It tells me what the first version should be prepared for.

- [x] ⭐ **Watching a screen share or camera of someone else (view only).** The most common thing phone users miss when friends on desktop share something.
- [ ] Admin and moderation basics (kick, ban, move people).
- [ ] Whisper and shout.
- [ ] File sharing.
- [ ] No preference yet.

✍️ Note:

#### Q1.9 How should the phone treat things it cannot show? `◉`
Example: someone in your channel starts a screen share, or sends a file through the relay.

- [x] ⭐ **Show a short, calm hint** such as "Anna is sharing her screen · available in the desktop app". Nobody wonders why the others are talking about something they cannot see.
- [ ] Hide it completely. Cleaner, but confusing in conversation.

✍️ Note:

---

### D · Phone-specific behaviour (principles; details follow in round 3)

#### Q1.10 Must voice keep running when the app is in the background or the screen is locked? `◉`
This is the hardest technical part on phones, especially on iOS. It needs a permanent "in a voice session" notification on Android and the call indicator on iOS.

- [x] ⭐ **Yes, a must-have for the first version.** Without it you cannot put the phone in your pocket, which makes a voice app pointless.
- [ ] Not in the first version; voice only while the app is open on screen. Much less work, but only useful as a demo.

✍️ Note:

#### Q1.11 Notifications when you are not in a voice session? `◉`
Important limit: Mumble has no push service, and we may not add anything on the server. So the phone only learns about a new message while the app itself holds a connection to the server. A fully closed app cannot be woken up by a message, on either platform.

- [x] ⭐ **Notifications only while you are connected.** While you are in a session (also in the background or with the screen locked) you get notifications for private messages and mentions. When you disconnect or the system ends the app, there are none, and the app says so clearly.
- [ ] Additionally an optional "stay connected in the background" switch for chat without voice. Works on Android (with a permanent notification and more battery use). On iOS the system does not allow it, so the two platforms would behave differently.
- [ ] No notifications in the first version.

✍️ Note:

---

### E · Identity and data

#### Q1.12 How does someone get onto a server with the same identity as on desktop? `☑`
Your identity is a certificate. Servers recognise you (registration, rights) by it, so the phone needs the same one to be "you".

- [x] ⭐ Import the `.p12` file exported from the desktop app (export already exists on desktop; the file is password-protected). It is stored in the secure storage of the phone (Android Keystore / iOS Keychain).
- [x] ⭐ Create a new identity on the phone, for people without a desktop identity.
- [ ] Later: transfer by scanning a QR code shown by the desktop app, without handling a file. More comfortable, needs new work on desktop too.

✍️ Note:

#### Q1.13 Should server list and settings be synchronised between desktop and phone? `◉`
A real sync would need an online account or service, which Mumcord does not have.

- [x] ⭐ **No sync.** The phone has its own server list and settings. Servers are added by hand.
- [ ] No sync, but a one-time import: the desktop app exports the server list to a file that the phone can read. Extra work on desktop.
- [ ] Later.

✍️ Note:

---

### F · Distribution and project organisation

#### Q1.14 How do Android users learn about a new version? `◉`
Outside the Play Store, Android does not update apps on its own. An app may download its new version, but the user always has to confirm the installation. On iOS, TestFlight handles updates by itself.

- [x] ⭐ **A notice inside the app ("Version X is available") that opens the download of the signed APK from the GitHub release.** Same trust chain as the desktop updater: only from our GitHub releases, checksum verified.
- [ ] No notice; people check the GitHub page themselves.
- [ ] Download and start the installation inside the app. More comfortable, needs an extra sensitive permission ("install unknown apps").

✍️ Note:

#### Q1.15 Who gets the TestFlight builds? `◉`
TestFlight has two modes. Internal: up to 100 invited people from your own Apple team, available right after upload. External: up to 10,000 people through a public link, but each first build of a version passes a short review by Apple.

- [x] ⭐ **Internal testers first, external link once the app is stable.** Fast start without Apple review; the public link follows as its own step inside this epic.
- [ ] External (public link) from the first build.
- [ ] Internal only in this epic.

✍️ Note:

#### Q1.16 Is there an Apple Developer Program membership for Mumcord? `◉`
Needed for any TestFlight build (99 USD per year, paid to Apple). This is the only thing in this epic that only you can set up. No recommendation here; please pick what is true.

- [x] Not yet, I will create one.
- [ ] Yes, it already exists.
- [ ] Not sure / explain what is needed.

✍️ Note:

#### Q1.17 Version numbers of the mobile apps? `◉`

- [x] ⭐ **Same version and same release as desktop.** One tag on `main` produces desktop installers, the APK and the TestFlight build. One changelog, no confusion about which versions fit together.
- [ ] Own version numbers and own release rhythm for mobile.

✍️ Note:

#### Q1.18 What happens to the earlier child issues #231–#245 and the earlier mockups? `◉`

- [x] ⭐ **Close them as superseded once the new spec exists; new child issues and new mockups are created from the spec.** The overview issue #228 is rewritten to point to the new ones. Clean start, no half-matching old texts.
- [ ] Keep the issue numbers and rewrite their texts to match the new spec.
- [ ] Decide after the spec is written.

✍️ Note:

---

## Round 1 – Answers (given in chat on 2026-11-13)

The product manager asked to answer in chat instead of in this file. The checkboxes above therefore still show the defaults; the answers below are the valid ones and win over the checkboxes.

- **Q1.1 Goal:** "Check which functions make sense." A feature assessment was presented in chat and the feature set was then fixed as listed under Q1.4/Q1.7 below. In short: more than a companion, less than a full replacement.
- **Q1.2 Platforms:** Both in this epic, Android leads.
- **Q1.3 Tablets:** Phones **and a real tablet layout** (two columns, like desktop). Differs from the recommendation.
- **Q1.4 / Q1.7 Feature set of the first mobile version:**
  - Basics: server list with favourites; connecting with server password, user name and certificate warnings; channel tree with people, join and switch, channel passwords; voice activity; on-screen push-to-talk; mute and deafen; per-person volume and local mute; channel chat with links and inline images; private messages; local chat history; mentions; identities (create, import); dark and light design; all 24 languages.
  - Added because they are cheap: whisper and shout; admin basics (kick, ban, move people, server mute); activity log; opening `mumble://` links.
  - Added by the product manager: watching a screen share of someone else; watching a camera of someone else; sharing the own phone camera; editing the own comment and avatar; custom themes; GIF picker.
  - Not in the first version: sharing the phone screen; file sharing; watch together; full admin tools (permissions editor, ban list, registered users, channel management); plugins.
  - Never on mobile: in-game overlay, positional audio for games, packet inspector, global hotkeys, tray icon, pop-out windows.
  - Emoji reactions: not applicable. The question was asked by mistake; desktop has no reactions.
- **Q1.5 Several servers:** One server at a time.
- **Q1.6 Default talk mode:** Ask on first start (short choice screen with both modes explained). Differs from the recommendation.
- **Q1.8 Next feature:** Obsolete; viewing screen shares and cameras is now part of the first version.
- **Q1.9 Things the phone cannot show:** Short, calm hint (applies to what remains unsupported: phone screen sharing by others is viewable, so mainly file transfers, watch together and plugin content).
- **Q1.10 Background and lock-screen voice:** Must-have (accepted as part of the feature set).
- **Q1.11 Notifications:** Only while connected.
- **Q1.12 Identity transfer:** `.p12` file import, create new, **and QR code transfer from the desktop app in the first version**. Needs new work on desktop.
- **Q1.13 Sync:** No ongoing sync, but a **one-time import of the server list by QR code** from the desktop app.
- **Q1.14 Android updates:** In-app notice that opens the download of the signed APK from the GitHub release.
- **Q1.15 TestFlight:** Internal testers first, public link once the app is stable.
- **Q1.16 Apple Developer Program:** A membership exists.
- **Q1.17 Versions:** Same version and same release as desktop.
- **Q1.18 Earlier issues and mockups:** Close as superseded once the new spec exists; new child issues and mockups come from the spec.

Consequences to carry into later rounds:
- Viewing streams and sharing the camera pull video decoding, video encoding and camera capture onto both phone platforms. This is the largest technical risk of the epic and is examined in round 4.
- QR transfer of identity and server list means the epic also contains desktop work (showing the code, securing the transfer). Asked in round 4.
- A real tablet layout means every screen is designed twice. Round 2 covers both.

---

## Round 2 – UI & UX (structure; asked and answered in chat on 2026-11-13)

The four structural questions were asked in chat with ASCII wireframes. All four answers follow the recommendation.

- **Q2.1 Navigation while connected:** Bottom tabs, always visible: Channels · Chat · Messages · More. Rejected: swipe panels (hidden gestures, weak for screen readers), menu drawer (two taps for every switch).
- **Q2.2 Voice controls:** A slim voice bar above the tabs on every screen (current channel, mute, deafen, leave). Tapping or swiping it up opens a full voice view with large buttons, the audio output choice, who is talking, and the large hold-to-talk button.
- **Q2.3 Tablet layout:** Two columns. Left: channel tree with people and the voice bar. Right: chat or stream. Works in portrait and landscape.
- **Q2.4 Streams of others (screen share, camera):** A live tile inside the channel; tap opens it full screen, rotating gives landscape, and going back shrinks it to a small floating window so that watching and chatting work together.

```text
Phone, connected                     Tablet, connected
┌──────────────────────┐             ┌───────────────┬──────────────────────────┐
│ voice.example.org  ⋮ │             │ voice.example │ #Lobby                   │
├──────────────────────┤             ├───────────────┤ Anna: hi all             │
│ ▾ Lobby           (3)│             │ ▾ Lobby    (3)│ Ben: hey                 │
│    🟢 Anna 🎙        │             │   🟢 Anna 🎙  │                          │
│    ⚪ Ben            │             │   ⚪ Ben      │                          │
│ ▸ Gaming          (5)│             │ ▸ Gaming   (5)│                          │
├──────────────────────┤             ├───────────────┤                          │
│ 🎙 Lobby   🔇 🎧  ✕  │ voice bar   │🎙 Lobby 🔇🎧 ✕│ Message #Lobby…       ➤  │
├──────────────────────┤             └───────────────┴──────────────────────────┘
│ Channels Chat  DMs  ⋯│ tabs
└──────────────────────┘
```

Mockups in dark and light are produced from these decisions. Remaining detail questions of round 2 are asked with the mockups in hand.

---

## Round 3 – Voice on a phone (asked and answered in chat on 2026-11-13)

- **Q3.1 Incoming phone call during a session:** Mumcord mutes and deafens for the length of the call and restores the previous state afterwards. You stay in the channel.
- **Q3.2 Should a session look like a phone call to the system?** **Yes, as a call.** Differs from the recommendation. Meaning: iOS call integration (CallKit) and the Android equivalent (a self-managed call with the "phone call" foreground service). Known consequences to handle in the spec: the session must be kept out of the "recent calls" list where the system allows it; behaviour with a real incoming call follows Q3.1; Apple restricts this technique in mainland China, which matters only for a later store release (Epic 6).
- **Q3.3 Push-to-talk with a locked screen:** Unlock to talk. Push-to-talk works on the open app only; listening always continues. Hands-free users choose voice activity.
- **Q3.4 Default audio output without a headset:** Ask the first time (loudspeaker or earpiece), remembered afterwards. A connected wired or Bluetooth headset always wins automatically.
- **Q3.5 Headset button:** One press toggles the microphone (mute / unmute), with a short sound as feedback.
- **Q3.6 Mobile data:** Configurable. Reading of the answer: settings for what happens on mobile data (streams start by themselves or on tap, inline images and GIFs, voice quality), with the careful defaults: streams on tap, the rest unchanged. To be confirmed in round 5.
- **Q3.7 Leaving a forgotten session:** Optional setting, off by default: "Disconnect when I'm alone in a channel for 30 minutes", **with a notification** when the 30 minutes are reached. Reading of the answer: the notification announces the disconnect and offers "Stay connected". To be confirmed in round 5.
- **Q3.8 Message text in notifications:** Sender and text go to the system and the phone's own lock-screen privacy setting decides; plus an in-app switch "show sender only".

---

## Round 4 – Technology & delivery (asked and answered in chat on 2026-11-13)

- **Q4.1 How the app is built:** **Native interfaces.** The Rust engine stays and is shared; the interface is written natively for each platform (Kotlin for Android, Swift for iOS). Differs from the recommendation (same stack as desktop with Tauri 2 mobile). Accepted consequences: best native feel; interface work is done per platform and every later mobile feature is built once per platform; the existing React interface is not reused, only its look, texts and behaviour rules.
- **Q4.2 Minimum system versions:** Android 12 and newer, iOS 17 and newer.
- **Q4.3 QR transfer of identity and server list:** Everything inside QR codes. The desktop app shows the encrypted data as a sequence of QR codes plus a short code that is typed on the phone. No network is involved. Differs from the recommendation (one-time key plus local Wi-Fi).
- **Q4.4 Automated testing:** Emulators in CI (Android emulator and iOS simulator against the local test server) plus a manual checklist on real phones per release for what emulators cannot prove (background voice, calls, Bluetooth, battery).

Decided by recommendation without a question, because they are purely technical and follow from the answers above:
- Interface toolkits: Jetpack Compose on Android, SwiftUI on iOS, each talking to the shared Rust engine through generated bindings.
- Crash reports: opt-in, as on desktop.
- Signing keys (Android keystore, Apple certificates and profiles) exist only as CI secrets, with an offline backup of the Android key kept by the product manager; losing that key would force every Android user to reinstall.

---

## Round 5 – Review & open points (asked and answered in chat on 2026-11-13)

- **Q5.1 Design with native interfaces:** One Mumcord design on both platforms (same colours, fonts, layout and icons, matching desktop). Only system pieces are platform-native: back gesture, share sheet, permission dialogs, keyboards. One set of mockups serves both apps.
- **Q5.2 Mobile data (confirms Q3.6):** Three switches with careful defaults: "Start streams automatically" off, "Load images and GIFs" on, "Reduced voice quality" off.
- **Q5.3 Forgotten session (confirms Q3.7):** Optional setting, off by default. After 30 minutes alone in a channel a notification says "Disconnecting in 1 minute" with a "Stay connected" button; without a tap, Mumcord leaves.

---

### Round 5, continued – repository and releases

The product manager asked whether the mobile apps can live in their own repository and use the engine as a library.

- **Q5.4 Own repository:** Yes. A new repository holds the Android app, the iOS app and the bridge to the engine. The engine comes from the Mumcord repository as a library, pinned to a release tag. Making the engine usable on phones and the desktop "Send to your phone" dialog stay in the Mumcord repository.
- **Q5.5 Versions and releases (replaces Q1.17):** The mobile apps have their own version numbers, starting at 0.1.0, and release whenever ready. Each mobile release states which engine version it contains.
- **Q5.6 Home of spec and issues:** The specification stays in the Mumcord repository. Engine and desktop tasks become issues in the Mumcord repository; app tasks become issues in the mobile repository; all are linked from #228.

---

### Round 5, addition – issue location

After Q5.6 the product manager instructed in chat to create the issues in the separate repository. Final rule: the specification stays in the Mumcord repository; **all** issues of this epic, including the engine and desktop tasks, are created in the mobile repository and labelled by area. Pull requests for engine and desktop tasks are still made in the Mumcord repository and reference those issues.

Correction noted while writing the specification: desktop has no GIF search (Spec 01 FR-1.52). "GIFs" on mobile therefore means inline playback and local GIF favourites, not a search picker.

---

## Closing

2026-11-13: All rounds answered. The specification has been created: `specs/05-mobile-app-specification.md`.

## Addendum 2026-11-14

Found during the first Android slice: on Android 17 (API 37, the app's target) an app cannot reach addresses on the directly connected local network, such as a Mumble server at home (192.168.x.x), without the runtime permission `ACCESS_LOCAL_NETWORK`. Without it the connection is dropped silently and the app shows "server could not be reached" after a timeout; internet servers are unaffected. iOS has a comparable local network privacy prompt that needs a usage description.
Decision (team lead, product manager informed): the app declares the permission. When the user connects to a local-network address and the permission is missing, the app explains in one sentence why it is needed, then triggers the system prompt. If denied, the connection error says so and offers the way to the system settings. Applies to Android and iOS. Recorded as FR-5.8 and AC-5.37.

## Addendum 2026-11-14: transfer code length

Found while building the transfer format for "Send to your phone" (FR-5.64): a 6-digit code has only a million possibilities. Someone who photographs all QR codes but not the number could try every code in roughly 5–10 hours on one desktop PC, in spite of the slow key derivation, and would then hold the identities for good. The three attempts and the two-minute limit only apply on the user's own phone.

Decision (product manager): the code has 8 digits, shown as two groups of four. That is a hundred times the effort (roughly three to four weeks on one PC, within a day on rented machines). The main protection stays that the codes and the number are only on the user's own screen for at most two minutes. The length is a setting of the sending side; the phone accepts 6 to 10 digits. Q4.3 (everything inside the QR codes plus a typed code) is unchanged.
