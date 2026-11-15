#!/bin/sh
# Tests the package-repository method of scripts/install.sh on any Linux machine, without root
# and without touching the system. MUMCORD_ROOT points every system path (/etc/apt, /etc/yum.repos.d,
# /etc/pacman.conf, /usr/local/etc/pkg ...) into a temporary folder, and fakes of curl, uname, id
# and the package managers record each call. It checks the method choice, the config files the
# installer writes for apt, dnf, zypper, pacman and FreeBSD pkg, the key hash check, the upgrade
# on a second run and that --uninstall removes everything again.
#
# What it cannot check: that a real package manager accepts the files and installs from the real
# repository. That needs the repository to be live (issue #620).
#
# Usage: check-repo.sh

set -eu

here=$(cd "$(dirname "$0")" && pwd)
installer=$here/../install.sh
published=$here/../../packaging/repo
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fpr=A4096249A499587C9AEF13E7F022C81D47D2BBEB

fail() {
    printf 'FAIL: %s\n' "$*" >&2
    exit 1
}
step() { printf '\n== %s\n' "$*"; }

# The "server": the published key files, and a tampered copy of the Linux key.
server=$work/server
mkdir -p "$server/good" "$server/bad"
cp "$published/mumcord.asc" "$published/mumcord.pub" "$server/good/"
cp "$published/mumcord.asc" "$published/mumcord.pub" "$server/bad/"
printf 'tampered' >>"$server/bad/mumcord.asc"

bin=$work/bin
mkdir -p "$bin"
for tool in sh cat chmod cp cut dirname env grep mkdir mktemp mv rm sed sleep tee touch tr wc sha256sum; do
    ln -s "$(command -v "$tool")" "$bin/$tool"
done
cat >"$bin/curl" <<EOF
#!/bin/sh
# curl -fsSL -o FILE URL, serving http://repo.invalid/ from the server folder.
src=$server/\${4#http://repo.invalid/}
[ -f "\$src" ] || exit 22
cp "\$src" "\$3"
EOF
cat >"$bin/uname" <<'EOF'
#!/bin/sh
case $1 in
-s) echo "$FAKE_OS" ;;
-r) echo 15.1-RELEASE ;;
-m) echo "$FAKE_MACHINE" ;;
esac
EOF
# Root, so the installer needs no sudo and the fakes are called directly.
printf '#!/bin/sh\necho 0\n' >"$bin/id"
# Package managers: every call that changes something is logged; queries say "not installed".
for tool in apt-get dnf zypper rpm pacman pacman-key pkg; do
    cat >"$bin/$tool" <<EOF
#!/bin/sh
case \${1:-} in -N | -Q | -q | query | info) exit 1 ;; esac
[ "\${1:-}" != -N ] || exit 0
echo "$tool \$*" >>"\$FAKE_LOG"
EOF
done
# pkg -N must succeed (pkg is bootstrapped); the generic fake above says no to it.
cat >"$bin/pkg" <<'EOF'
#!/bin/sh
case ${1:-} in
-N) exit 0 ;;
query | info) exit 1 ;;
esac
echo "pkg $*" >>"$FAKE_LOG"
EOF
printf '#!/bin/sh\nexit 1\n' >"$bin/dpkg-query"
(cd "$bin" && chmod 755 curl uname id apt-get dnf zypper rpm pacman pacman-key pkg dpkg-query)

shell=$(command -v sh)
root=$work/root
log=$work/log

# reset FAMILY-ID [existing pacman.conf text]: a clean fake system of one distribution.
reset() {
    rm -rf "$root" "$log" "${work:?}/home"
    mkdir -p "$root/etc" "$work/home"
    : >"$log"
    printf 'ID=%s\n' "$1" >"$root/etc/os-release"
    [ -z "${2:-}" ] || printf '%s\n' "$2" >"$root/etc/pacman.conf"
}

# run [VAR=VALUE...] [-- ARGS...]: runs the installer as the fake system; leaves the output in
# $out and the status in $status.
run() {
    vars=
    while [ $# -gt 0 ] && [ "$1" != -- ]; do
        vars="$vars $1"
        shift
    done
    [ $# -eq 0 ] || shift
    # shellcheck disable=SC2086 # one word per VAR=VALUE
    out=$(env -i HOME="$work/home" PATH="$bin" FAKE_LOG="$log" FAKE_OS=Linux FAKE_MACHINE=x86_64 \
        MUMCORD_ROOT="$root" MUMCORD_REPO_URL=http://repo.invalid/good $vars \
        "$shell" "$installer" "$@" 2>&1) && status=0 || status=$?
    printf '%s\n' "$out"
}

expect_output() { printf '%s\n' "$out" | grep -qF -- "$1" || fail "output lacks: $1"; }
expect_log() { grep -qxF -- "$1" "$log" || fail "no call: $1 (log: $(cat "$log"))"; }
expect_file() { [ -f "$1" ] || fail "missing file: $1"; }
expect_line() { grep -qxF -- "$2" "$1" || fail "$1 lacks the line: $2"; }
expect_no_change() { [ ! -s "$log" ] || fail "something changed: $(cat "$log")"; }
no_repo_files() {
    for file in etc/apt/sources.list.d/mumcord.sources etc/apt/keyrings/mumcord.asc etc/yum.repos.d/mumcord.repo \
        etc/zypp/repos.d/mumcord.repo usr/local/etc/pkg/keys/mumcord.pub usr/local/etc/pkg/repos/mumcord.conf; do
        [ ! -e "$root/$file" ] || fail "$file exists $1"
    done
    ! grep -qs '^\[mumcord\]' "$root/etc/pacman.conf" || fail "pacman.conf still has [mumcord] $1"
}

step "debian: repository is the default with --yes"
reset ubuntu
run -- --yes --no-desktop-icon
[ "$status" -eq 0 ] || fail "the installer failed"
expect_file "$root/etc/apt/keyrings/mumcord.asc"
cmp -s "$published/mumcord.asc" "$root/etc/apt/keyrings/mumcord.asc" || fail "the apt key is not the published key"
src=$root/etc/apt/sources.list.d/mumcord.sources
expect_line "$src" "URIs: http://repo.invalid/good/apt"
expect_line "$src" "Signed-By: /etc/apt/keyrings/mumcord.asc"
expect_line "$src" "Suites: stable"
expect_log "apt-get update"
expect_log "apt-get install -y mumcord"
expect_output "now come from apt"

step "debian: uninstall removes the repository setup"
run -- --uninstall
[ "$status" -eq 0 ] || fail "--uninstall failed"
no_repo_files "after --uninstall"
expect_output "Mumcord removed"

step "fedora: --method repo"
reset fedora
run -- --yes --no-desktop-icon --method repo
[ "$status" -eq 0 ] || fail "the installer failed"
grep -q '^rpm --import .*/mumcord\.asc$' "$log" || fail "key not imported (log: $(cat "$log"))"
repo=$root/etc/yum.repos.d/mumcord.repo
expect_line "$repo" "baseurl=http://repo.invalid/good/rpm"
expect_line "$repo" "gpgcheck=1"
expect_line "$repo" "repo_gpgcheck=1"
expect_log "dnf install -y mumcord"
expect_output "now come from dnf"
run -- --uninstall
no_repo_files "after --uninstall"

step "opensuse: MUMCORD_INSTALL_METHOD=repo"
reset opensuse-tumbleweed
run MUMCORD_INSTALL_METHOD=repo -- --yes --no-desktop-icon
[ "$status" -eq 0 ] || fail "the installer failed"
expect_line "$root/etc/zypp/repos.d/mumcord.repo" "baseurl=http://repo.invalid/good/rpm"
[ ! -e "$root/etc/yum.repos.d/mumcord.repo" ] || fail "zypper setup wrote the dnf file"
grep -q '^rpm --import ' "$log" || fail "key not imported"
expect_log "zypper --non-interactive refresh"
expect_log "zypper --non-interactive install mumcord"
run -- --uninstall
no_repo_files "after --uninstall"

step "arch: block appended once, no partial upgrade, upgrade on a second run"
reset arch "[options]
HoldPkg = pacman glibc
[core]
Include = /etc/pacman.d/mirrorlist"
run -- --yes --no-desktop-icon
[ "$status" -eq 0 ] || fail "the installer failed"
grep -q '^pacman-key --add .*/mumcord\.asc$' "$log" || fail "key not added"
expect_log "pacman-key --lsign-key $fpr"
expect_log "pacman -Syu --needed --noconfirm mumcord-bin"
! grep -q '^pacman -Sy ' "$log" || fail "partial upgrade (-Sy)"
conf=$root/etc/pacman.conf
expect_line "$conf" "[mumcord]"
# shellcheck disable=SC2016 # $arch is for pacman to expand
expect_line "$conf" 'Server = http://repo.invalid/good/arch/$arch'
run -- --yes --no-desktop-icon
[ "$(grep -c '^\[mumcord\]$' "$conf")" -eq 1 ] || fail "second run added the block again"
[ "$(grep -c '^pacman -Syu --needed' "$log")" -eq 2 ] || fail "second run did not upgrade"
expect_output "now come from pacman"
run -- --uninstall
no_repo_files "after --uninstall"
expect_line "$conf" "[core]"
expect_line "$conf" "Include = /etc/pacman.d/mirrorlist"
expect_log "pacman-key --delete $fpr"

step "freebsd: key, repo conf, pkg update and install"
reset irrelevant
run FAKE_OS=FreeBSD FAKE_MACHINE=amd64 -- --yes --no-desktop-icon
[ "$status" -eq 0 ] || fail "the installer failed"
cmp -s "$published/mumcord.pub" "$root/usr/local/etc/pkg/keys/mumcord.pub" || fail "the pkg key is not the published key"
bsd=$root/usr/local/etc/pkg/repos/mumcord.conf
# shellcheck disable=SC2016 # ${ABI} is for pkg to expand
expect_line "$bsd" '  url: "http://repo.invalid/good/freebsd/${ABI}",'
expect_line "$bsd" '  pubkey: "/usr/local/etc/pkg/keys/mumcord.pub",'
expect_log "pkg update"
expect_log "pkg install -y mumcord"
expect_output "now come from pkg"
run FAKE_OS=FreeBSD FAKE_MACHINE=amd64 -- --uninstall
no_repo_files "after --uninstall"

step "tampered key aborts and changes nothing"
reset ubuntu
run MUMCORD_REPO_URL=http://repo.invalid/bad -- --yes --no-desktop-icon
[ "$status" -ne 0 ] || fail "installer succeeded with a wrong key hash"
expect_output "not the one this script expects"
expect_no_change
no_repo_files "after the key mismatch"

step "unsuitable methods are refused"
reset gentoo
run -- --yes --method repo
[ "$status" -ne 0 ] || fail "repo accepted on an unsupported distribution"
expect_output "--method repo is not available on this system"
expect_no_change
reset fedora
run -- --yes --method nonsense
[ "$status" -ne 0 ] || fail "an unknown method was accepted"
expect_output "must be repo, package or appimage"
reset irrelevant
run FAKE_OS=FreeBSD FAKE_MACHINE=amd64 -- --yes --method appimage
[ "$status" -ne 0 ] || fail "AppImage accepted on FreeBSD"
expect_output "--method appimage is not available on this system"

step "uninstall without anything installed"
reset ubuntu
run -- --uninstall
[ "$status" -eq 0 ] || fail "--uninstall failed"
expect_output "not installed"

printf '\nPASS (repository method, faked)\n'
