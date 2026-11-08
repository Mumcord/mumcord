#!/bin/sh
# Runs inside a distribution container and tests scripts/install.sh against a fake release
# (make-fake-release.sh) served over http: a tampered checksum aborts without installing,
# `curl | sh` installs the expected kind of package with a desktop icon, --uninstall removes
# everything and a second --uninstall finds nothing. Output that is not a terminal must be plain
# text without escape codes. For the AppImage it also checks the output in a pseudo-terminal:
# colours and Unicode marks, none with TERM=dumb or NO_COLOR, and no gst-libav question (every
# package brings its own H.264 decoder).
#
# Usage: check-install.sh deb|rpm|appimage GOOD_URL BAD_URL
#   GOOD_URL serves the fake release, BAD_URL the same files with a wrong checksum.

set -eu

expected=$1
good=$2
bad=$3
desktop=$HOME/Desktop/Mumcord.desktop
appimage=$HOME/.local/bin/Mumcord.AppImage
entry=$HOME/.local/share/applications/app.mumcord.client.desktop

esc=$(printf '\033')

fail() {
    printf 'FAIL: %s\n' "$*" >&2
    exit 1
}
step() { printf '\n== %s\n' "$*"; }

assert_nothing_installed() {
    [ ! -e /usr/bin/mumcord ] || fail "/usr/bin/mumcord exists ($1)"
    [ ! -e "$appimage" ] || fail "$appimage exists ($1)"
    [ ! -e "$entry" ] || fail "$entry exists ($1)"
    [ ! -e "$desktop" ] || fail "$desktop exists ($1)"
}

step "tampered checksum aborts"
if out=$(curl -fsSL "$bad/install.sh" | MUMCORD_RELEASE_URL=$bad sh -s -- --yes --desktop-icon 2>&1); then
    printf '%s\n' "$out"
    fail "installer succeeded with a wrong checksum"
fi
printf '%s\n' "$out"
printf '%s\n' "$out" | grep -q "checksum mismatch" || fail "no checksum mismatch message"
assert_nothing_installed "after the checksum mismatch"

step "install with curl | sh"
out=$(curl -fsSL "$good/install.sh" | MUMCORD_RELEASE_URL=$good TERM=xterm-256color sh -s -- --yes --desktop-icon 2>&1)
printf '%s\n' "$out"
! printf '%s\n' "$out" | grep -qF "$esc" || fail "escape codes in output that is not a terminal"
case $expected in
deb)
    dpkg-query -W mumcord >/dev/null || fail "the .deb is not installed"
    [ ! -e "$appimage" ] || fail "AppImage installed instead of the .deb"
    ;;
rpm)
    rpm -q Mumcord >/dev/null || fail "the .rpm is not installed"
    [ ! -e "$appimage" ] || fail "AppImage installed instead of the .rpm"
    ;;
appimage)
    [ -x "$appimage" ] || fail "AppImage not installed"
    grep -qx 'Exec="'"$appimage"'" %u' "$entry" || fail "menu entry does not start the AppImage"
    grep -qx 'X-Mumcord-Managed=true' "$entry" || fail "menu entry lacks the managed marker"
    grep -qx 'MimeType=x-scheme-handler/mumble;' "$entry" || fail "menu entry is not the mumble:// handler"
    [ -f "$HOME/.local/share/icons/hicolor/128x128/apps/app.mumcord.client.png" ] || fail "icon missing"
    ;;
esac
[ "$expected" = appimage ] || [ -x /usr/bin/mumcord ] || fail "/usr/bin/mumcord missing"
[ -f "$desktop" ] || fail "desktop icon missing"

# tty_run [VAR=VALUE...]: runs the installer in a pseudo-terminal (script(1)) with the given
# environment, answers no to any question and prints what the terminal showed.
tty_run() {
    printf 'n\n' | script -qec "env $* sh $installer --no-desktop-icon" /dev/null 2>&1
}

if [ "$expected" = appimage ]; then
    installer=$(mktemp)
    curl -fsSL -o "$installer" "$good/install.sh"
    export MUMCORD_RELEASE_URL="$good"

    step "terminal: colours and Unicode marks, no gst-libav question"
    out=$(tty_run TERM=xterm-256color LANG=C.UTF-8)
    printf '%s\n' "$out"
    printf '%s\n' "$out" | grep -qF "$esc" || fail "no colours in a terminal"
    printf '%s\n' "$out" | grep -qF "✓" || fail "no Unicode marks in a UTF-8 terminal"
    ! printf '%s\n' "$out" | grep -qF "gst-libav" || fail "gst-libav question or hint"

    step "TERM=dumb: plain text"
    out=$(tty_run TERM=dumb)
    printf '%s\n' "$out"
    ! printf '%s\n' "$out" | grep -qF "$esc" || fail "escape codes despite TERM=dumb"

    step "NO_COLOR: plain text"
    out=$(tty_run TERM=xterm-256color LANG=C.UTF-8 NO_COLOR=1)
    printf '%s\n' "$out"
    ! printf '%s\n' "$out" | grep -qF "$esc" || fail "escape codes despite NO_COLOR"

    unset MUMCORD_RELEASE_URL
fi

step "uninstall"
curl -fsSL "$good/install.sh" | sh -s -- --uninstall
assert_nothing_installed "after --uninstall"

step "second uninstall finds nothing"
out=$(curl -fsSL "$good/install.sh" | sh -s -- --uninstall)
printf '%s\n' "$out"
printf '%s\n' "$out" | grep -q "not installed" || fail "second --uninstall did not report 'not installed'"

printf '\nPASS (%s)\n' "$expected"
