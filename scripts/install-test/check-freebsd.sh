#!/bin/sh
# Tests the FreeBSD path of scripts/install.sh on any Linux machine, without FreeBSD and without
# installing anything. The installer runs with a PATH that holds only basic tools and fakes of
# uname, id, fetch, sha256, pkg, sudo and doas: the fakes record every pkg call and serve a fake
# release from a local folder, so the test checks which commands the installer would run, the
# platform and version checks and the messages. No curl, wget or sha256sum is on that PATH, so
# the download and the checksum must use FreeBSD's fetch and sha256 -q.
#
# What it cannot check: that a real pkg resolves and installs the dependencies, that the real
# package installs, and how the real fetch, sha256 and doas behave. That needs FreeBSD 15.x
# (AC-13.1 of specs/13-freebsd-support-specification.md).
#
# Usage: check-freebsd.sh

set -eu

here=$(cd "$(dirname "$0")" && pwd)
installer=$here/../install.sh
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fail() {
    printf 'FAIL: %s\n' "$*" >&2
    exit 1
}
step() { printf '\n== %s\n' "$*"; }

# The fake release folders: good, tampered (bad) and one from before 0.8.0 (old).
release=$work/release
mkdir -p "$release/good" "$release/old"
printf 'fake FreeBSD package' >"$release/good/mumcord-0.8.0.pkg"
printf 'fake deb' >"$release/good/Mumcord_0.8.0_amd64.deb"
(cd "$release/good" && sha256sum -- * >SHA256SUMS)
cp -R "$release/good" "$release/bad"
printf 'tampered' >>"$release/bad/mumcord-0.8.0.pkg"
printf 'fake deb' >"$release/old/Mumcord_0.7.2_amd64.deb"
(cd "$release/old" && sha256sum -- * >SHA256SUMS)

# Basic tools the installer needs, linked from this machine; everything else is a fake.
bin=$work/bin
mkdir -p "$bin"
for tool in sh cat chmod cp cut env grep mkdir mktemp mv rm sed sleep touch tr wc; do
    ln -s "$(command -v "$tool")" "$bin/$tool"
done
real_sha256sum=$(command -v sha256sum)

cat >"$bin/uname" <<'EOF'
#!/bin/sh
case $1 in
-s) echo FreeBSD ;;
-r) echo "$FAKE_RELEASE" ;;
-m) echo "$FAKE_MACHINE" ;;
esac
EOF
cat >"$bin/id" <<'EOF'
#!/bin/sh
echo 1000
EOF
# fetch -q -o FILE URL, serving http://release.invalid/ from the release folder.
cat >"$bin/fetch" <<EOF
#!/bin/sh
src=$release/\${4#http://release.invalid/}
[ -f "\$src" ] || exit 1
cp "\$src" "\$3"
EOF
cat >"$bin/sha256" <<EOF
#!/bin/sh
[ "\$1" = -q ] || exit 2
echo "sha256 -q" >>"\$FAKE_LOG"
"$real_sha256sum" "\$2" | cut -d ' ' -f 1
EOF
for tool in sudo doas; do
    cat >"$bin/$tool" <<EOF
#!/bin/sh
ELEVATED=$tool exec "\$@"
EOF
done
# pkg keeps its state in FAKE_STATE: "bootstrapped" and "installed" (the installed version).
cat >"$bin/pkg" <<'EOF'
#!/bin/sh
case $1 in
-N) [ -f "$FAKE_STATE/bootstrapped" ] && exit 0 || exit 1 ;;
query)
    if [ "$2" = -F ]; then
        printf 'gtk3\nwebkit2-gtk_41\n'
    elif [ -f "$FAKE_STATE/installed" ]; then
        cat "$FAKE_STATE/installed"
    else
        exit 1
    fi
    exit 0
    ;;
info) [ -f "$FAKE_STATE/installed" ] && exit 0 || exit 1 ;;
esac
# Every call that changes the system is recorded with who ran it.
printf '%s pkg %s\n' "${ELEVATED:-user}" "$*" >>"$FAKE_LOG"
case $1 in
bootstrap) [ "${ASSUME_ALWAYS_YES:-}" = yes ] || exit 1 && touch "$FAKE_STATE/bootstrapped" ;;
add) echo 0.8.0 >"$FAKE_STATE/installed" ;;
delete) rm -f "$FAKE_STATE/installed" ;;
esac
EOF
(cd "$bin" && chmod 755 uname id fetch sha256 sudo doas pkg)

shell=$(command -v sh)
log=$work/log
state=$work/state

# run RELEASE [VAR=VALUE...] [-- ARGS...]: runs the installer as a fresh FreeBSD 15.1 amd64 user
# against a release folder; prints its output, leaves the status in $status.
run() {
    folder=$1
    shift
    vars=
    while [ $# -gt 0 ] && [ "$1" != -- ]; do
        vars="$vars $1"
        shift
    done
    [ $# -eq 0 ] || shift
    # shellcheck disable=SC2086 # one word per VAR=VALUE
    out=$(env -i HOME="$work/home" PATH="$bin" FAKE_LOG="$log" FAKE_STATE="$state" \
        FAKE_RELEASE=15.1-RELEASE FAKE_MACHINE=amd64 \
        MUMCORD_RELEASE_URL="http://release.invalid/$folder" $vars \
        "$shell" "$installer" --yes --no-desktop-icon "$@" 2>&1) && status=0 || status=$?
    printf '%s\n' "$out"
}

# reset [bootstrapped] [VERSION]: a clean fake system.
reset() {
    rm -rf "$state" "$log" "${work:?}/home"
    mkdir -p "$state" "$work/home"
    : >"$log"
    [ "${1:-}" != bootstrapped ] || touch "$state/bootstrapped"
    [ -z "${2:-}" ] || echo "$2" >"$state/installed"
}

expect_output() { printf '%s\n' "$out" | grep -qF -- "$1" || fail "output lacks: $1"; }
expect_log() { grep -qxF -- "$1" "$log" || fail "no call: $1 (log: $(cat "$log"))"; }
expect_no_change() { ! grep -q ' pkg ' "$log" || fail "pkg changed the system: $(cat "$log")"; }

for platform in "14.3-RELEASE amd64" "15.1-RELEASE arm64" "16.0-CURRENT amd64"; do
    step "unsupported: FreeBSD $platform"
    reset bootstrapped
    run good "FAKE_RELEASE=${platform% *}" "FAKE_MACHINE=${platform#* }"
    [ "$status" -ne 0 ] || fail "installer succeeded on $platform"
    expect_output "FreeBSD 15.x on amd64 only, not FreeBSD ${platform% *} on ${platform#* }"
    expect_output "Nothing was changed"
    expect_no_change
done

step "release before 0.8.0"
reset bootstrapped
run old
[ "$status" -ne 0 ] || fail "installer succeeded without a FreeBSD package"
expect_output "FreeBSD packages exist from Mumcord 0.8.0 on"
expect_no_change

step "tampered checksum aborts"
reset bootstrapped
run bad
[ "$status" -ne 0 ] || fail "installer succeeded with a wrong checksum"
expect_output "checksum mismatch"
expect_log "sha256 -q"
expect_no_change

step "no sudo and no doas"
reset bootstrapped
mv "$bin/sudo" "$bin/doas" "$work/"
run good
mv "$work/sudo" "$work/doas" "$bin/"
[ "$status" -ne 0 ] || fail "installer succeeded without a way to become root"
expect_output "neither sudo nor doas is installed"
expect_no_change

step "install with doas on a fresh system"
reset
mv "$bin/sudo" "$work/"
run good
mv "$work/sudo" "$bin/"
[ "$status" -eq 0 ] || fail "the installer failed"
expect_log "doas pkg bootstrap"
expect_log "doas pkg install -y gtk3 webkit2-gtk_41"
grep -qx "doas pkg add .*/mumcord-0\.8\.0\.pkg" "$log" || fail "no pkg add of the package (log: $(cat "$log"))"
[ "$(grep -n 'pkg install' "$log" | cut -d: -f1)" -lt "$(grep -n 'pkg add' "$log" | cut -d: -f1)" ] ||
    fail "dependencies were not installed before the package"
expect_output "Mumcord 0.8.0 is installed"
expect_output "/usr/local/bin/mumcord"

step "install with sudo"
reset bootstrapped
run good
[ "$status" -eq 0 ] || fail "the installer failed"
expect_log "sudo pkg install -y gtk3 webkit2-gtk_41"
grep -q "^sudo pkg add " "$log" || fail "pkg add did not run with sudo"
! grep -q "pkg bootstrap" "$log" || fail "bootstrapped pkg again"

step "same version already installed"
reset bootstrapped 0.8.0
run good
[ "$status" -eq 0 ] || fail "the installer failed"
expect_output "already installed"
! grep -q "pkg add" "$log" || fail "pkg add ran for the installed version"

step "update from an older version"
reset bootstrapped 0.7.2
run good
[ "$status" -eq 0 ] || fail "the installer failed"
grep -q "^sudo pkg add -f .*/mumcord-0\.8\.0\.pkg$" "$log" || fail "no pkg add -f (log: $(cat "$log"))"
expect_output "Mumcord 0.7.2 replaced by 0.8.0"

step "uninstall"
reset bootstrapped 0.8.0
run good -- --uninstall
[ "$status" -eq 0 ] || fail "--uninstall failed"
expect_log "sudo pkg delete -y mumcord"
expect_output "Mumcord removed"

step "uninstall when not installed"
reset bootstrapped
run good -- --uninstall
[ "$status" -eq 0 ] || fail "--uninstall failed"
expect_output "not installed"
expect_no_change

printf '\nPASS (FreeBSD, faked)\n'
