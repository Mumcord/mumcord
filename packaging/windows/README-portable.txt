Mumcord portable for Windows
============================

Mumcord is a voice chat client compatible with Mumble. This zip runs without
installing: unpack it anywhere (a USB stick, your Documents folder, a PC
without administrator rights) and start Mumcord.exe.

Portable mode
-------------
The empty file named "portable" next to Mumcord.exe switches portable mode on.
Mumcord then keeps everything in the "MumcordData" folder next to the exe:

  MumcordData\data   servers, settings, chat history, plugins
  MumcordData\local  the web view's data and the caches
  MumcordData\logs   the log files

Nothing is written to %APPDATA% or %LOCALAPPDATA%. Mumcord does not register
itself for mumble:// links, does not start at login and creates no Start menu
or desktop shortcuts unless you turn these on yourself in the first-start
wizard or in Settings. Delete the "portable" file to use the normal data
folders of an installed Mumcord instead.

Your identity
-------------
Your identity certificate (how servers recognise you) and saved server
passwords are kept in the Windows Credential Manager of the Windows user who
runs Mumcord, not in MumcordData: a private key in a plain file on a stick
would be easy to copy. The identity is therefore per Windows user, even in
portable mode. To take it to another PC or user, use Settings > Identity >
Export identity (a password-protected file) and Import identity there.

Requirements
------------
Windows 10 or 11 (64-bit) with the Microsoft Edge WebView2 Runtime. Windows 11
and most Windows 10 PCs have it. If it is missing, Mumcord says so and offers
the download: https://go.microsoft.com/fwlink/p/?LinkId=2124703

Updates
-------
Mumcord tells you when a new version exists. Download the new portable zip and
unpack it into this folder, replacing Mumcord.exe; MumcordData stays as it is.

Licences
--------
Mumcord is licensed under the GNU Affero General Public License, version 3
only (LICENSE). NOTICE and THIRD_PARTY_NOTICES list the third-party components
and their licences. Source code: https://github.com/Mumcord/mumcord
