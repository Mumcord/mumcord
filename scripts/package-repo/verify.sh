#!/bin/bash
# Checks a built repository the way a user's system uses it, inside a fresh distribution
# container (issue #620 acceptance criteria): a changed index (port 8781) and a changed package
# (port 8782) are rejected, then Mumcord installs from the real repository (port 8780) with the
# published config files and without warnings. tamper.sh makes the damaged copies; the caller
# serves the three sites on 127.0.0.1 and mounts packaging/repo at /cfg.
#
# Usage (as root): verify.sh apt|dnf|zypper|pacman
set -euo pipefail

family=$1
public=https://packages.mumcord.app
local=http://127.0.0.1

expect_fail() {
  if "$@" > /tmp/out 2>&1; then
    cat /tmp/out
    echo "::error::A tampered repository was accepted: $*" >&2
    exit 1
  fi
  echo "Rejected as expected: $*"
}

# Ports of the three sites: real, changed index, changed package.
good=8780 bad_meta=8781 bad_pkg=8782

case $family in
  apt)
    export DEBIAN_FRONTEND=noninteractive
    install -D -m 644 /cfg/mumcord.asc /etc/apt/keyrings/mumcord.asc
    use() { sed "s#$public#$local:$1#" /cfg/mumcord.sources > /etc/apt/sources.list.d/mumcord.sources; }
    # apt-get update exits 0 on some repository errors; any warning or error counts as failure.
    update() {
      local out
      out=$(apt-get update 2>&1) || { echo "$out"; return 1; }
      echo "$out"
      ! grep -qE '^(W|E):' <<< "$out"
    }
    use $bad_meta
    expect_fail update
    use $bad_pkg
    update
    expect_fail apt-get install -y mumcord
    use $good
    update
    apt-get install -y mumcord
    dpkg-query -W mumcord
    ;;
  dnf | zypper)
    # The documented first step: import the key (its fingerprint is in the docs).
    rpm --import /cfg/mumcord.asc
    if [[ $family == dnf ]]; then
      repo=/etc/yum.repos.d/mumcord.repo
      refresh() { dnf -y makecache --refresh; }
      add() { dnf -y install "$@"; }
    else
      repo=/etc/zypp/repos.d/mumcord.repo
      refresh() { zypper --non-interactive refresh -f mumcord; }
      add() { zypper --non-interactive install "$@"; }
    fi
    use() { sed "s#$public#$local:$1#" /cfg/mumcord.repo > "$repo"; }
    # dnf reports a bad repomd.xml signature but may still exit 0 and skip the repository, so
    # what counts is that nothing installs from it.
    use $bad_meta
    expect_fail add mumcord
    use $bad_pkg
    refresh
    expect_fail add mumcord
    use $good
    refresh
    add mumcord
    rpm -q mumcord
    ;;
  pacman)
    pacman-key --init >/dev/null
    pacman-key --add /cfg/mumcord.asc
    pacman-key --lsign-key "$(gpg --batch --with-colons --show-keys /cfg/mumcord.asc | awk -F: '/^fpr/ { print $10; exit }')"
    # shellcheck disable=SC2016 # $arch is pacman's variable, not the shell's.
    printf '\n[mumcord]\nServer = %s:%s/arch/$arch\n' "$local" "$good" >> /etc/pacman.conf
    use() { sed -i "s#^Server = $local:[0-9]*/arch#Server = $local:$1/arch#" /etc/pacman.conf; }
    use $bad_meta
    expect_fail pacman -Syy --noconfirm
    use $bad_pkg
    pacman -Syy --noconfirm
    expect_fail pacman -Syu --noconfirm mumcord-bin
    use $good
    pacman -Syy --noconfirm
    pacman -Syu --noconfirm mumcord-bin
    pacman -Q mumcord-bin
    ;;
  *)
    echo "Unknown family $family" >&2
    exit 2
    ;;
esac
echo "Repository verified for $family."
