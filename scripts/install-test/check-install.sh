#!/bin/sh
# Runs inside a distribution container and tests scripts/install.sh against a fake release
# (make-fake-release.sh) served over http: a tampered checksum aborts without installing,
# `curl | sh` installs the expected kind of package with a desktop icon, --uninstall removes
# everything and a second --uninstall finds nothing.
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
out=$(curl -fsSL "$good/install.sh" | MUMCORD_RELEASE_URL=$good sh -s -- --yes --desktop-icon 2>&1)
printf '%s\n' "$out"
case $expected in
deb)
    dpkg-query -W mumcord >/dev/null || fail "the .deb is not installed"
    [ ! -e "$appimage" ] || fail "AppImage installed instead of the .deb"
    ! printf '%s\n' "$out" | grep -qF "Some videos need H.264 support" || fail "codec hint shown for a native package"
    ;;
rpm)
    rpm -q Mumcord >/dev/null || fail "the .rpm is not installed"
    [ ! -e "$appimage" ] || fail "AppImage installed instead of the .rpm"
    ! printf '%s\n' "$out" | grep -qF "Some videos need H.264 support" || fail "codec hint shown for a native package"
    ;;
appimage)
    [ -x "$appimage" ] || fail "AppImage not installed"
    grep -qx 'Exec="'"$appimage"'" %u' "$entry" || fail "menu entry does not start the AppImage"
    grep -qx 'X-Mumcord-Managed=true' "$entry" || fail "menu entry lacks the managed marker"
    grep -qx 'MimeType=x-scheme-handler/mumble;' "$entry" || fail "menu entry is not the mumble:// handler"
    [ -f "$HOME/.local/share/icons/hicolor/128x128/apps/app.mumcord.client.png" ] || fail "icon missing"
    printf '%s\n' "$out" | grep -qF "Some videos need H.264 support" || fail "no codec hint after the AppImage install"
    ;;
esac
[ "$expected" = appimage ] || [ -x /usr/bin/mumcord ] || fail "/usr/bin/mumcord missing"
[ -f "$desktop" ] || fail "desktop icon missing"

step "uninstall"
curl -fsSL "$good/install.sh" | sh -s -- --uninstall
assert_nothing_installed "after --uninstall"

step "second uninstall finds nothing"
out=$(curl -fsSL "$good/install.sh" | sh -s -- --uninstall)
printf '%s\n' "$out"
printf '%s\n' "$out" | grep -q "not installed" || fail "second --uninstall did not report 'not installed'"

printf '\nPASS (%s)\n' "$expected"
