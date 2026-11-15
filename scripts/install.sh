#!/bin/sh
# Installs Mumcord on Linux or FreeBSD from the latest GitHub release:
#
#   curl -fsSL https://github.com/Mumcord/mumcord/releases/latest/download/install.sh | sh
#   fetch -qo - https://github.com/Mumcord/mumcord/releases/latest/download/install.sh | sh
#
# You choose how to install (the default is the first one that fits your system):
#   repository  adds Mumcord's signed package repository (https://packages.mumcord.app) to
#               apt, dnf, zypper, pacman or pkg and installs from it, so updates come with your
#               system updates. Running the script again upgrades.
#   package     downloads the release package: the .deb (apt), .rpm (dnf/zypper), the AUR package
#               on Arch when an AUR helper is installed and the AUR has it, or the FreeBSD 15.x
#               .pkg on amd64 (Mumcord 0.8.0 and later).
#   appimage    the AppImage for the current user with an app-menu entry (Linux only). Every
#               other Linux gets only this one.
# Every download is checked against a checksum before anything from it runs: packages against
# the release's SHA256SUMS, the repository keys against a hash inside this script. sudo (or
# doas) is used only to install or remove software and repository settings.
#
# Options (after `sh -s --` when piped):
#   --method repo|package|appimage   how to install (see above); asked when omitted
#   --uninstall          remove Mumcord and its repository setup (settings and certificate stay)
#   --desktop-icon       add a desktop icon without asking
#   --no-desktop-icon    do not add a desktop icon and do not ask
#   --yes                ask nothing and use the defaults (for scripts and CI)
# Environment: MUMCORD_INSTALL_METHOD=repo|package|appimage, MUMCORD_DESKTOP_ICON=yes|no,
# MUMCORD_NONINTERACTIVE=1, MUMCORD_RELEASE_URL to download from another release folder and
# MUMCORD_REPO_URL to use another package repository (tests use a local server).
# Colours and the progress spinner appear only on a terminal; NO_COLOR or TERM=dumb turn them off.

set -eu

RELEASE_URL=${MUMCORD_RELEASE_URL:-https://github.com/Mumcord/mumcord/releases/latest/download}
APP_ID=app.mumcord.client
BIN_DIR=$HOME/.local/bin
DATA_HOME=${XDG_DATA_HOME:-$HOME/.local/share}
APPIMAGE=$BIN_DIR/Mumcord.AppImage
APPLICATIONS=$DATA_HOME/applications
ENTRY=$APPLICATIONS/$APP_ID.desktop
# Same marker as src-tauri/src/desktop_entry.rs: the app keeps such an entry up to date and
# treats it as the user's yes to the menu entry.
MANAGED_KEY=X-Mumcord-Managed=true
# Line present in every Mumcord menu entry (package or AppImage); identifies desktop icons.
WM_CLASS_LINE=StartupWMClass=$APP_ID
AUR_PACKAGE=mumcord-bin
ICON_SIZES="32x32 128x128 256x256 512x512"
# The FreeBSD package and the release it is built for (specs/13-freebsd-support-specification.md).
FREEBSD_PACKAGE=mumcord
FREEBSD_MAJOR=15
OS=$(uname -s)
# The signed package repository (specs: issue #620). The key hashes pin the published keys, so a
# changed key file on the server is never trusted; the *_SHA256 overrides exist for tests only.
REPO_URL=${MUMCORD_REPO_URL:-https://packages.mumcord.app}
REPO_KEY_FPR=A4096249A499587C9AEF13E7F022C81D47D2BBEB
REPO_KEY_SHA256=${MUMCORD_REPO_KEY_SHA256:-7d85c44e4f245bef0d52f129957d12ec8fbbd7f118950e97076478bf54439c6e}
REPO_PUBKEY_SHA256=${MUMCORD_REPO_PUBKEY_SHA256:-9c03cc83f882d84192c8489d070c519ac2ec190c7eb61a53161913d330aeafa6}
REPO_PACKAGE=mumcord
# Prefix for every system path the repository setup touches; empty except in tests.
ROOT=${MUMCORD_ROOT:-}
APT_KEY=$ROOT/etc/apt/keyrings/mumcord.asc
APT_SOURCES=$ROOT/etc/apt/sources.list.d/mumcord.sources
DNF_REPO=$ROOT/etc/yum.repos.d/mumcord.repo
ZYPP_REPO=$ROOT/etc/zypp/repos.d/mumcord.repo
PACMAN_CONF=$ROOT/etc/pacman.conf
BSD_KEY=$ROOT/usr/local/etc/pkg/keys/mumcord.pub
BSD_CONF=$ROOT/usr/local/etc/pkg/repos/mumcord.conf

# Colours, Unicode marks and the spinner only for a terminal that wants them
# (https://no-color.org); pipes, log files and TERM=dumb get plain ASCII lines.
fancy=false
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-dumb}" != dumb ]; then
    fancy=true
fi
unicode=false
case ${LC_ALL:-${LC_CTYPE:-${LANG:-}}} in
*UTF-8* | *utf-8* | *UTF8* | *utf8*) unicode=$fancy ;;
esac
ESC=$(printf '\033')
ACCENT='' BOLD='' DIM='' GREEN='' RED='' YELLOW='' RESET=''
if $fancy; then
    # The app's accent colour (#a78bff), or the closest of the 256 colours.
    case ${COLORTERM:-} in
    truecolor | 24bit) ACCENT="${ESC}[1;38;2;167;139;255m" ;;
    *) ACCENT="${ESC}[1;38;5;141m" ;;
    esac
    BOLD="${ESC}[1m" DIM="${ESC}[2m" GREEN="${ESC}[1;32m" RED="${ESC}[1;31m" YELLOW="${ESC}[1;33m" RESET="${ESC}[0m"
fi
# Errors and warnings go to stderr, which may be a log file even when stdout is a terminal.
ERR_RED='' ERR_YELLOW='' ERR_RESET=''
if $fancy && [ -t 2 ]; then
    ERR_RED=$RED ERR_YELLOW=$YELLOW ERR_RESET=$RESET
fi
if $unicode; then
    MARK_OK=✓ MARK_FAIL=✗ MARK_WARN=! MARK_SKIP=– MARK_ASK='?'
    BOX_TL=╭ BOX_TR=╮ BOX_BL=╰ BOX_BR=╯ BOX_H=─ BOX_V=│
    INFO_PAD='         '
else
    MARK_OK='OK  ' MARK_FAIL=FAIL MARK_WARN=WARN MARK_SKIP=SKIP MARK_ASK='?'
    BOX_TL=+ BOX_TR=+ BOX_BL=+ BOX_BR=+ BOX_H=- BOX_V='|'
    INFO_PAD='            '
fi

say() { printf '%s\n' "$*"; }
# Secondary lines under a step, such as what sudo is about to do.
info() { printf '%s%s%s%s\n' "$INFO_PAD" "$DIM" "$*" "$RESET"; }
warn() { printf '  %s%s%s Warning: %s\n' "$ERR_YELLOW" "$MARK_WARN" "$ERR_RESET" "$*" >&2; }
die() {
    clear_line
    printf '  %s%s%s Error: %s\n' "$ERR_RED" "$MARK_FAIL" "$ERR_RESET" "$*" >&2
    exit 1
}

# Erases a spinner line, so the next line starts clean.
clear_line() {
    if $fancy; then printf '\r%s[K' "$ESC"; fi
}

step_no=0
# step ok|fail|warn|skip LABEL DETAIL: one numbered line of the install progress.
step() {
    step_no=$((step_no + 1))
    case $1 in
    ok) mark=$MARK_OK colour=$GREEN ;;
    fail) mark=$MARK_FAIL colour=$RED ;;
    warn) mark=$MARK_WARN colour=$YELLOW ;;
    *) mark=$MARK_SKIP colour=$DIM ;;
    esac
    clear_line
    printf '  %s%2d%s  %s%s%s  %s%-14s%s %s\n' "$DIM" "$step_no" "$RESET" "$colour" "$mark" "$RESET" "$BOLD" "$2" "$RESET" "$3"
}

banner() {
    say
    if $unicode; then
        printf '  %s┌┬┐┬ ┬┌┬┐┌─┐┌─┐┬─┐┌┬┐%s\n' "$ACCENT" "$RESET"
        printf '  %s││││ │││││  │ │├┬┘ ││%s   %s%s%s\n' "$ACCENT" "$RESET" "$BOLD" "$1" "$RESET"
        printf '  %s┴ ┴└─┘┴ ┴└─┘└─┘┴└──┴┘%s   %svoice chat for Mumble servers%s\n' "$ACCENT" "$RESET" "$DIM" "$RESET"
    else
        printf '  %sMumcord%s %s\n' "$ACCENT" "$RESET" "$1"
    fi
    say
}

# repeat TEXT COUNT: prints TEXT COUNT times.
repeat() {
    i=0
    while [ "$i" -lt "$2" ]; do
        printf '%s' "$1"
        i=$((i + 1))
    done
}

# box TITLE LINE...: the summary at the end, framed to the longest line.
box() {
    width=0
    for line in "$@"; do
        len=$(printf '%s' "$line" | wc -m)
        [ "$len" -le "$width" ] || width=$len
    done
    say
    printf '  %s%s%s%s\n' "$ACCENT" "$BOX_TL" "$(repeat "$BOX_H" $((width + 4)))" "$BOX_TR$RESET"
    first=true
    for line in "$@"; do
        len=$(printf '%s' "$line" | wc -m)
        pad=$(repeat ' ' $((width - len)))
        if $first; then
            printf '  %s%s%s  %s%s%s%s  %s%s%s\n' "$ACCENT" "$BOX_V" "$RESET" "$BOLD" "$line" "$RESET" "$pad" "$ACCENT" "$BOX_V" "$RESET"
            first=false
        else
            printf '  %s%s%s  %s%s  %s%s%s\n' "$ACCENT" "$BOX_V" "$RESET" "$line" "$pad" "$ACCENT" "$BOX_V" "$RESET"
        fi
    done
    printf '  %s%s%s%s\n' "$ACCENT" "$BOX_BL" "$(repeat "$BOX_H" $((width + 4)))" "$BOX_BR$RESET"
    say
}

# human_size BYTES: "12.3 MB" or "512 KB" (rounded up, so a small file is not "0 KB").
human_size() {
    if [ "$1" -ge 1048576 ]; then
        printf '%d.%d MB' $(($1 / 1048576)) $(($1 % 1048576 * 10 / 1048576))
    else
        printf '%d KB' $((($1 + 1023) / 1024))
    fi
}

file_size() {
    if [ -f "$1" ]; then
        wc -c <"$1" | tr -d ' '
    else
        echo 0
    fi
}
have() { command -v "$1" >/dev/null 2>&1; }

usage() {
    sed -n '2,32s/^# \{0,1\}//p' "$0" 2>/dev/null || say "See https://github.com/Mumcord/mumcord#download"
}

uninstall=false
method=${MUMCORD_INSTALL_METHOD:-}
desktop_icon=${MUMCORD_DESKTOP_ICON:-ask}
noninteractive=${MUMCORD_NONINTERACTIVE:-}
while [ $# -gt 0 ]; do
    arg=$1
    shift
    case $arg in
    --method)
        [ $# -gt 0 ] || die "--method needs repo, package or appimage"
        method=$1
        shift
        ;;
    --method=*) method=${arg#--method=} ;;
    --uninstall) uninstall=true ;;
    --desktop-icon) desktop_icon=yes ;;
    --no-desktop-icon) desktop_icon=no ;;
    --yes) noninteractive=1 ;;
    -h | --help)
        usage
        exit 0
        ;;
    *) die "unknown option: $arg (see --help)" ;;
    esac
done
case $desktop_icon in
yes | no | ask) ;;
*) die "MUMCORD_DESKTOP_ICON must be yes or no" ;;
esac
case $method in
'' | repo | package | appimage) ;;
*) die "the install method must be repo, package or appimage, not $method" ;;
esac

# Questions go to the terminal, because with `curl | sh` standard input is the script itself.
can_ask() {
    [ -z "$noninteractive" ] && (: </dev/tty) 2>/dev/null
}

# ask_yes_no QUESTION: true on yes; the default (no) when nobody can answer.
ask_yes_no() {
    can_ask || return 1
    printf '  %s%s%s %s [y/N] ' "$ACCENT" "$MARK_ASK" "$RESET" "$1" >/dev/tty
    read -r answer </dev/tty || return 1
    case $answer in
    [yY] | [yY][eE][sS]) return 0 ;;
    *) return 1 ;;
    esac
}

SUDO=
root_checked=false
need_root() {
    if $root_checked; then
        return
    fi
    root_checked=true
    if [ "$(id -u)" -eq 0 ]; then
        SUDO=
        return
    fi
    # FreeBSD often has doas instead of sudo.
    for tool in sudo doas; do
        if have "$tool"; then
            info "$1 needs administrator rights; $tool may ask for your password."
            SUDO=$tool
            return
        fi
    done
    die "$1 needs administrator rights, but neither sudo nor doas is installed. Run this script as root instead."
}

download() {
    if have curl; then
        curl -fsSL -o "$2" "$1"
    elif have wget; then
        wget -q -O "$2" "$1"
    elif have fetch; then
        # FreeBSD's base system has fetch(1), but neither curl nor wget.
        fetch -q -o "$2" "$1"
    else
        die "curl, wget or fetch is needed to download Mumcord"
    fi
}

# download_progress URL FILE LABEL: download with a spinner and the size so far on a terminal.
download_progress() {
    if ! $fancy; then
        say "$3 ..."
        download "$1" "$2"
        return
    fi
    have curl || have wget || have fetch || die "curl, wget or fetch is needed to download Mumcord"
    # The shell cannot tell a finished background job from a running one without waiting, so the
    # job leaves its exit status in a file.
    rm -f "$work/download.status"
    (
        status=0
        download "$1" "$2" || status=$?
        echo "$status" >"$work/download.status"
    ) &
    download_pid=$!
    frame=0
    printf '%s[?25l' "$ESC"
    while [ ! -s "$work/download.status" ]; do
        if $unicode; then
            case $frame in
            0) spin=⠋ ;; 1) spin=⠙ ;; 2) spin=⠹ ;; 3) spin=⠸ ;; 4) spin=⠼ ;;
            5) spin=⠴ ;; 6) spin=⠦ ;; 7) spin=⠧ ;; 8) spin=⠇ ;; *) spin=⠏ ;;
            esac
        else
            case $((frame % 4)) in
            0) spin='|' ;; 1) spin=/ ;; 2) spin=- ;; *) spin="\\" ;;
            esac
        fi
        frame=$(((frame + 1) % 10))
        printf '\r%s[K  %s%2d%s  %s%s%s  %s  %s%s%s' "$ESC" "$DIM" $((step_no + 1)) "$RESET" "$ACCENT" "$spin" "$RESET" "$3" \
            "$DIM" "$(human_size "$(file_size "$2")")" "$RESET"
        sleep 0.1 2>/dev/null || sleep 1
    done
    wait "$download_pid" 2>/dev/null || true
    download_pid=
    clear_line
    printf '%s[?25h' "$ESC"
    return "$(cat "$work/download.status")"
}

sha256_of() {
    # FreeBSD's base sha256(1) prints only the hash with -q.
    if [ "$OS" = FreeBSD ] && have sha256; then
        sha256 -q "$1"
    elif have sha256sum; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif have shasum; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        die "sha256sum is needed to check the download"
    fi
}

# Prints "debian", "fedora", "suse", "arch" or "other".
distro_family() {
    ids=$(
        # shellcheck disable=SC1091
        [ -r "$ROOT/etc/os-release" ] && . "$ROOT/etc/os-release" && printf '%s %s' "${ID:-}" "${ID_LIKE:-}"
    ) || ids=
    for id in $ids; do
        case $id in
        debian | ubuntu) have apt-get && echo debian && return ;;
        fedora | rhel | centos) have dnf && echo fedora && return ;;
        suse | opensuse | opensuse-*) have zypper && echo suse && return ;;
        arch) echo arch && return ;;
        esac
    done
    echo other
}

cpu() {
    case $(uname -m) in
    x86_64 | amd64) echo x86_64 ;;
    aarch64 | arm64) echo aarch64 ;;
    *) die "Mumcord is available for x86_64 and ARM64 only, not $(uname -m)" ;;
    esac
}

# Regular expression for the release asset of a package kind and CPU. The version part only
# allows plain characters, so a name from SHA256SUMS can never point outside the download folder.
asset_pattern() {
    v='[0-9A-Za-z.+~-]+'
    case $1-$2 in
    deb-x86_64) echo "^Mumcord_${v}_amd64\\.deb\$" ;;
    deb-aarch64) echo "^Mumcord_${v}_arm64\\.deb\$" ;;
    rpm-x86_64) echo "^Mumcord-${v}\\.x86_64\\.rpm\$" ;;
    rpm-aarch64) echo "^Mumcord-${v}\\.aarch64\\.rpm\$" ;;
    appimage-x86_64) echo "^Mumcord_${v}_amd64\\.AppImage\$" ;;
    appimage-aarch64) echo "^Mumcord_${v}_aarch64\\.AppImage\$" ;;
    pkg-x86_64) echo "^${FREEBSD_PACKAGE}-${v}\\.pkg\$" ;;
    esac
}

# fetch_asset KIND: downloads the matching asset into $work, checks it and sets $fetched to its
# path and $version to the release version. It runs in this shell, not in $(...), so its step
# lines reach the terminal and the step count goes on. (Not named fetch: that is FreeBSD's
# download tool, which download() calls.)
fetch_asset() {
    sums=$work/SHA256SUMS
    download "$RELEASE_URL/SHA256SUMS" "$sums" || die "could not download the checksum list from $RELEASE_URL"
    pattern=$(asset_pattern "$1" "$(cpu)")
    # sha256sum writes "hash  name" (or "hash *name" in binary mode).
    matches=$(
        while read -r hash file rest; do
            [ -z "$rest" ] || continue
            file=${file#\*}
            printf '%s\n' "$file" | grep -Eq "$pattern" && printf '%s %s\n' "$hash" "$file"
        done <"$sums"
    ) || true
    if [ -z "$matches" ]; then
        [ "$1" != pkg ] || die "this release has no FreeBSD package; FreeBSD packages exist from Mumcord 0.8.0 on. Nothing was installed."
        die "the release has no $1 package for $(cpu)"
    fi
    [ "$(printf '%s\n' "$matches" | wc -l)" -eq 1 ] || die "the release lists more than one $1 package for $(cpu)"
    expected=${matches% *}
    name=${matches#* }
    download_progress "$RELEASE_URL/$name" "$work/$name" "Downloading $name" || {
        step fail Download "$name"
        die "could not download $name"
    }
    step ok Download "$name ($(human_size "$(file_size "$work/$name")"))"
    actual=$(sha256_of "$work/$name")
    if [ "$actual" != "$expected" ]; then
        rm -f "$work/$name"
        step fail Checksum "does not match SHA256SUMS"
        die "checksum mismatch for $name: the download is damaged or was tampered with. Nothing was installed."
    fi
    step ok Checksum "OK, matches SHA256SUMS"
    fetched=$work/$name
    # Mumcord_0.4.3_amd64.deb, Mumcord-0.4.3-1.x86_64.rpm, Mumcord_0.4.3_amd64.AppImage,
    # mumcord-0.8.0.pkg
    case $name in
    Mumcord_*) version=${name#Mumcord_} version=${version%%_*} ;;
    "$FREEBSD_PACKAGE"-*) version=${name#"$FREEBSD_PACKAGE"-} version=${version%.pkg} ;;
    *) version=${name#Mumcord-} version=${version%.*.rpm} version=${version%-*} ;;
    esac
}

desktop_dir() {
    dir=
    have xdg-user-dir && dir=$(xdg-user-dir DESKTOP 2>/dev/null) || dir=
    printf '%s\n' "${dir:-$HOME/Desktop}"
}

# The package's or the user's menu entry, whichever exists.
menu_entry() {
    for file in "$ENTRY" /usr/share/applications/Mumcord.desktop /usr/share/applications/$APP_ID.desktop \
        /usr/local/share/applications/$APP_ID.desktop; do
        [ -f "$file" ] && grep -qx "$WM_CLASS_LINE" "$file" && printf '%s\n' "$file" && return
    done
    return 1
}

offer_desktop_icon() {
    case $desktop_icon in
    no)
        step skip "Desktop icon" "not added"
        return
        ;;
    ask)
        ask_yes_no "Add a desktop icon?" || {
            step skip "Desktop icon" "not added"
            return 0
        }
        ;;
    esac
    source_entry=$(menu_entry) || {
        step warn "Desktop icon" "no Mumcord menu entry found; skipping the desktop icon"
        return
    }
    dir=$(desktop_dir)
    mkdir -p "$dir"
    cp "$source_entry" "$dir/Mumcord.desktop"
    chmod 755 "$dir/Mumcord.desktop"
    # GNOME only starts desktop files that are marked trusted.
    if have gio; then
        gio set "$dir/Mumcord.desktop" metadata::trusted true 2>/dev/null || true
    fi
    step ok "Desktop icon" "added to $dir"
}

# Quotes a path for Exec= exactly like exec_program in src-tauri/src/desktop_entry.rs, so the
# app finds the entry up to date and does not rewrite it.
exec_program() {
    printf '"%s"' "$(printf '%s' "$1" | sed -e 's/[\\"`$]/\\&/g' -e 's/\\/\\\\/g')"
}

refresh_menu() {
    if have update-desktop-database; then
        update-desktop-database "$APPLICATIONS" 2>/dev/null || warn "update-desktop-database failed"
    fi
}

install_appimage() {
    fetch_asset appimage
    file=$fetched
    mkdir -p "$BIN_DIR" "$APPLICATIONS"
    chmod 755 "$file"
    mv -f "$file" "$APPIMAGE"
    # The AppImage was verified above; extracting needs no FUSE and runs nothing else.
    if (cd "$work" && "$APPIMAGE" --appimage-extract Mumcord.png >/dev/null 2>&1) && [ -f "$work/squashfs-root/Mumcord.png" ]; then
        icon_dir=$DATA_HOME/icons/hicolor/128x128/apps
        mkdir -p "$icon_dir"
        cp "$work/squashfs-root/Mumcord.png" "$icon_dir/$APP_ID.png"
    else
        warn "could not read the icon from the AppImage; Mumcord adds it on first start"
    fi
    cat >"$ENTRY" <<EOF
[Desktop Entry]
Type=Application
Name=Mumcord
Comment=Voice chat compatible with Mumble
Exec=$(exec_program "$APPIMAGE") %u
Icon=$APP_ID
Terminal=false
Categories=Network;Chat;AudioVideo;
MimeType=x-scheme-handler/mumble;
$WM_CLASS_LINE
$MANAGED_KEY
EOF
    refresh_menu
    step ok Install "Mumcord installed to $APPIMAGE"
    step ok "App menu" "added to the app menu"
    installed_path=$APPIMAGE
    case ":$PATH:" in
    *":$BIN_DIR:"*) start_hint="from the app menu, or run Mumcord.AppImage" ;;
    *) start_hint="from the app menu ($BIN_DIR is not in your PATH)" ;;
    esac
}

install_package() {
    case $1 in
    debian)
        fetch_asset deb
        file=$fetched
        # apt reads local packages as an unprivileged user, so the file must be readable.
        chmod 755 "$work"
        chmod 644 "$file"
        need_root "Installing the package"
        $SUDO apt-get install -y "$file" </dev/null || die "apt-get could not install $file"
        ;;
    fedora)
        fetch_asset rpm
        file=$fetched
        need_root "Installing the package"
        $SUDO dnf install -y "$file" </dev/null || die "dnf could not install $file"
        ;;
    suse)
        fetch_asset rpm
        file=$fetched
        need_root "Installing the package"
        # The release's .rpm files are not GPG-signed; the SHA256SUMS check above vouches for them.
        $SUDO zypper --non-interactive install --allow-unsigned-rpm "$file" </dev/null || die "zypper could not install $file"
        ;;
    esac
    step ok Install "Mumcord installed"
    installed_path=/usr/bin/mumcord
    start_hint="from the app menu, or run mumcord"
}

# Stops with what is supported unless this is FreeBSD 15.x on amd64. Runs before anything is
# downloaded or installed.
check_freebsd() {
    release=$(uname -r)
    machine=$(uname -m)
    if [ "$machine" != amd64 ] || [ "${release%%.*}" != "$FREEBSD_MAJOR" ]; then
        die "Mumcord for FreeBSD is available for FreeBSD $FREEBSD_MAJOR.x on amd64 only, not FreeBSD $release on $machine. Nothing was changed."
    fi
}

# A fresh system has only the pkg bootstrap stub, which would ask before installing pkg.
bootstrap_pkg() {
    if ! pkg -N >/dev/null 2>&1; then
        $SUDO env ASSUME_ALWAYS_YES=yes pkg bootstrap </dev/null || die "could not set up pkg"
    fi
}

install_freebsd() {
    fetch_asset pkg
    file=$fetched
    need_root "Installing the package"
    bootstrap_pkg
    # pkg add looks for missing dependencies only next to the package file, and the release has
    # none there, so they come from the configured repositories first.
    deps=$(pkg query -F "$file" '%dn') || die "could not read the dependencies of $file"
    if [ -n "$deps" ]; then
        deps=$(printf '%s\n' "$deps" | tr '\n' ' ')
        info "Installing the packages Mumcord needs from the pkg repositories."
        # shellcheck disable=SC2086 # one word per package name
        $SUDO pkg install -y $deps </dev/null || die "pkg could not install the packages Mumcord needs: $deps"
        step ok Dependencies "$(printf '%s' "$deps" | wc -w | tr -d ' ') packages from the pkg repositories"
    fi
    installed=$(pkg query '%v' "$FREEBSD_PACKAGE" 2>/dev/null) || installed=
    if [ "$installed" = "$version" ]; then
        step skip Install "Mumcord $version is already installed"
    elif [ -n "$installed" ]; then
        # Not every pkg version lets pkg add replace an installed version; -f always does.
        $SUDO pkg add -f "$file" </dev/null || die "pkg add could not install $file"
        step ok Install "Mumcord $installed replaced by $version"
    else
        $SUDO pkg add "$file" </dev/null || die "pkg add could not install $file"
        step ok Install "Mumcord installed"
    fi
    installed_path=/usr/local/bin/mumcord
    start_hint="from the app menu, or run mumcord"
}

# --- Package repository ---------------------------------------------------------------------

# fetch_repo_key NAME HASH: downloads NAME from the repository into $work and checks its hash.
# The hash is inside this script, because gpg may not be installed and the key must not be
# trusted just because the server sent it.
fetch_repo_key() {
    download "$REPO_URL/$1" "$work/$1" || die "could not download $REPO_URL/$1"
    actual=$(sha256_of "$work/$1")
    if [ "$actual" != "$2" ]; then
        rm -f "$work/$1"
        step fail "Signing key" "does not match the expected hash"
        die "the repository key $1 is not the one this script expects. Nothing was installed."
    fi
    step ok "Signing key" "$1 downloaded, hash OK"
}

# put_file SRC DEST: installs a world-readable file as administrator, creating the folder.
put_file() {
    if ! { $SUDO mkdir -p "$(dirname "$2")" && $SUDO cp "$1" "$2" && $SUDO chmod 644 "$2"; }; then
        die "could not write $2"
    fi
}

# write_repo_file DEST: reads the new content from standard input.
write_repo_file() {
    cat >"$work/repo-file"
    put_file "$work/repo-file" "$1"
    step ok "Repository" "added $(short_path "${1#"$ROOT"}")"
}

# update_hint_for MANAGER: what the summary says about updates.
repo_update_hint() {
    update_hint="now come from $1 with your system updates"
}

# installed_version PACKAGE: best-effort version of an installed package, for the summary.
installed_version() {
    case $family in
    debian) dpkg-query -W -f '${Version}' "$1" 2>/dev/null ;;
    fedora | suse) rpm -q --qf '%{VERSION}' "$1" 2>/dev/null ;;
    arch) found=$(pacman -Q "$1" 2>/dev/null) && printf '%s' "${found#* }" ;;
    freebsd) pkg query '%v' "$1" 2>/dev/null ;;
    esac || true
}

install_repo() {
    pkg_name=$REPO_PACKAGE
    case $1 in
    debian)
        need_root "Adding the package repository"
        fetch_repo_key mumcord.asc "$REPO_KEY_SHA256"
        put_file "$work/mumcord.asc" "$APT_KEY"
        write_repo_file "$APT_SOURCES" <<EOF
Types: deb
URIs: $REPO_URL/apt
Suites: stable
Components: main
Architectures: amd64 arm64
Signed-By: /etc/apt/keyrings/mumcord.asc
EOF
        $SUDO apt-get update </dev/null || die "apt-get update failed"
        # Installing an already installed package upgrades it to the newest version.
        $SUDO apt-get install -y "$pkg_name" </dev/null || die "apt-get could not install $pkg_name"
        repo_update_hint apt
        ;;
    fedora | suse)
        need_root "Adding the package repository"
        fetch_repo_key mumcord.asc "$REPO_KEY_SHA256"
        $SUDO rpm --import "$work/mumcord.asc" || die "could not import the repository key"
        repo_file=$DNF_REPO
        [ "$1" != suse ] || repo_file=$ZYPP_REPO
        write_repo_file "$repo_file" <<EOF
[mumcord]
name=Mumcord
type=rpm-md
baseurl=$REPO_URL/rpm
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=$REPO_URL/mumcord.asc
EOF
        if [ "$1" = fedora ]; then
            $SUDO dnf install -y "$pkg_name" </dev/null || die "dnf could not install $pkg_name"
            repo_update_hint dnf
        else
            $SUDO zypper --non-interactive refresh </dev/null || die "zypper could not refresh the repositories"
            $SUDO zypper --non-interactive install "$pkg_name" </dev/null || die "zypper could not install $pkg_name"
            repo_update_hint zypper
        fi
        ;;
    arch)
        pkg_name=$AUR_PACKAGE
        need_root "Adding the package repository"
        fetch_repo_key mumcord.asc "$REPO_KEY_SHA256"
        $SUDO pacman-key --add "$work/mumcord.asc" </dev/null || die "could not add the repository key"
        $SUDO pacman-key --lsign-key "$REPO_KEY_FPR" </dev/null || die "could not trust the repository key"
        if grep -qx '\[mumcord\]' "$PACMAN_CONF" 2>/dev/null; then
            step skip "Repository" "[mumcord] is already in pacman.conf"
        else
            # $arch is for pacman to expand.
            # shellcheck disable=SC2016
            printf '\n[mumcord]\nServer = %s/arch/$arch\n' "$REPO_URL" | $SUDO tee -a "$PACMAN_CONF" >/dev/null ||
                die "could not write $PACMAN_CONF"
            step ok "Repository" "added [mumcord] to pacman.conf"
        fi
        # -Syu, never -Sy alone: a partial upgrade can break Arch.
        if can_ask; then
            $SUDO pacman -Syu --needed "$pkg_name" </dev/tty || die "pacman could not install $pkg_name"
        else
            $SUDO pacman -Syu --needed --noconfirm "$pkg_name" </dev/null || die "pacman could not install $pkg_name"
        fi
        repo_update_hint pacman
        ;;
    freebsd)
        need_root "Adding the package repository"
        fetch_repo_key mumcord.pub "$REPO_PUBKEY_SHA256"
        put_file "$work/mumcord.pub" "$BSD_KEY"
        # ${ABI} is for pkg to expand.
        # shellcheck disable=SC2016
        write_repo_file "$BSD_CONF" <<EOF
mumcord: {
  url: "$REPO_URL/freebsd/\${ABI}",
  signature_type: "pubkey",
  pubkey: "/usr/local/etc/pkg/keys/mumcord.pub",
  enabled: yes
}
EOF
        bootstrap_pkg
        $SUDO pkg update </dev/null || die "pkg update failed"
        $SUDO pkg install -y "$pkg_name" </dev/null || die "pkg could not install $pkg_name"
        repo_update_hint pkg
        ;;
    esac
    step ok Install "Mumcord installed from the package repository"
    version=$(installed_version "$pkg_name")
    if [ "$1" = freebsd ]; then installed_path=/usr/local/bin/mumcord; else installed_path=/usr/bin/mumcord; fi
    start_hint="from the app menu, or run mumcord"
}

# choose_method FAMILY: sets $method from --method, the question or the default.
choose_method() {
    avail="repo package appimage"
    case $1 in
    other) avail=appimage ;;
    freebsd) avail="repo package" ;;
    esac
    if [ -n "$method" ]; then
        case " $avail " in
        *" $method "*) return ;;
        esac
        die "--method $method is not available on this system (available: $(printf '%s' "$avail" | tr ' ' ','))"
    fi
    if [ "$avail" = appimage ]; then
        method=appimage
        return
    fi
    method=repo
    can_ask || return 0
    # The menu names this system's own tools, so the choice is concrete.
    case $1 in
    debian) tool=apt package_label='.deb package from the release: you update it by hand' ;;
    fedora) tool=dnf package_label='.rpm package from the release: you update it by hand' ;;
    suse) tool=zypper package_label='.rpm package from the release: you update it by hand' ;;
    arch) tool=pacman package_label='AUR package mumcord-bin: updates come with your AUR helper' ;;
    *) tool=pkg package_label='.pkg package from the release: run this script again to update' ;;
    esac
    {
        printf '  %s%s%s How do you want to install Mumcord?\n' "$ACCENT" "$MARK_ASK" "$RESET"
        printf '      1) Package repository: updates come with %s (recommended)\n' "$tool"
        printf '      2) %s\n' "$package_label"
        case $avail in
        *appimage*) printf '      3) AppImage in your home folder: updates itself, no root needed\n' ;;
        esac
        printf '  Choose [1]: '
    } >/dev/tty
    read -r answer </dev/tty || return 0
    case $answer in
    2 | package) method=package ;;
    3 | appimage)
        case $avail in
        *appimage*) method=appimage ;;
        esac
        ;;
    esac
}

aur_helper() {
    for helper in paru yay; do
        have "$helper" && echo "$helper" && return
    done
    return 1
}

# The AUR package may not exist yet; then the AppImage is used instead.
aur_has_package() {
    "$1" -Si "$AUR_PACKAGE" >/dev/null 2>&1 </dev/null
}

do_install() {
    banner installer
    version='' installed_path='' start_hint='' aur_tip=false update_hint=''
    if [ "$OS" = FreeBSD ]; then
        family=freebsd
        check_freebsd
    else
        family=$(distro_family)
        cpu >/dev/null
    fi
    choose_method "$family"
    case $method in
    repo) install_repo "$family" ;;
    appimage) install_appimage ;;
    *) install_by_package "$family" ;;
    esac
    offer_desktop_icon
    summary
}

install_by_package() {
    case $1 in
    freebsd) install_freebsd ;;
    debian | fedora | suse) install_package "$family" ;;
    arch)
        if helper=$(aur_helper) && aur_has_package "$helper"; then
            info "Installing $AUR_PACKAGE from the AUR with $helper."
            if can_ask; then
                "$helper" -S --needed "$AUR_PACKAGE" </dev/tty || die "$helper could not install $AUR_PACKAGE"
            else
                "$helper" -S --needed --noconfirm "$AUR_PACKAGE" </dev/null || die "$helper could not install $AUR_PACKAGE"
            fi
            step ok Install "$AUR_PACKAGE from the AUR"
            found=$(pacman -Q "$AUR_PACKAGE" 2>/dev/null) && version=${found#* }
            installed_path=/usr/bin/mumcord
            start_hint="from the app menu, or run mumcord"
        else
            install_appimage
            aur_tip=true
        fi
        ;;
    esac
}

# short_path PATH: PATH with the home folder written as ~.
short_path() {
    case $1 in
    "$HOME"/*) printf '%s/%s' '~' "${1#"$HOME"/}" ;;
    *) printf '%s' "$1" ;;
    esac
}

summary() {
    set -- "Mumcord${version:+ $version} is installed" "" \
        "Path    $(short_path "$installed_path")" \
        "Start   $start_hint"
    if [ -n "$update_hint" ]; then
        set -- "$@" "Updates $update_hint"
    fi
    if $aur_tip; then
        set -- "$@" "Tip     if the AUR has $AUR_PACKAGE, installing it with an AUR" \
            "        helper gets Mumcord updates with your system."
    fi
    box "$@"
}

# installed_package NAME...: prints the first name the package manager knows as installed.
installed_package() {
    for pkg in "$@"; do
        if have dpkg-query && [ "$(dpkg-query -W -f '${Status}' "$pkg" 2>/dev/null)" = "install ok installed" ]; then
            echo "deb $pkg" && return
        fi
        if have rpm && rpm -q "$pkg" >/dev/null 2>&1; then
            echo "rpm $pkg" && return
        fi
        if have pacman && pacman -Q "$pkg" >/dev/null 2>&1; then
            echo "pacman $pkg" && return
        fi
        if [ "$OS" = FreeBSD ] && have pkg && pkg info -e "$pkg" 2>/dev/null; then
            echo "pkg $pkg" && return
        fi
    done
    return 1
}

# remove_repo_setup: deletes the repository settings and keys the installer added.
remove_repo_setup() {
    files=
    for file in "$APT_SOURCES" "$APT_KEY" "$DNF_REPO" "$ZYPP_REPO" "$BSD_CONF" "$BSD_KEY"; do
        [ -f "$file" ] && files="$files $file"
    done
    pacman_block=false
    if grep -qx '\[mumcord\]' "$PACMAN_CONF" 2>/dev/null; then
        pacman_block=true
    fi
    if [ -z "$files" ] && ! $pacman_block; then
        return
    fi
    need_root "Removing the repository setup"
    if [ -n "$files" ]; then
        # shellcheck disable=SC2086 # one word per path; none contains spaces
        $SUDO rm -f $files
    fi
    if $pacman_block; then
        # The block is the [mumcord] line up to its Server line.
        sed '/^\[mumcord\]$/,/^Server/d' "$PACMAN_CONF" >"$work/pacman.conf"
        $SUDO cp "$work/pacman.conf" "$PACMAN_CONF" || warn "could not edit $PACMAN_CONF"
        if have pacman-key; then
            $SUDO pacman-key --delete "$REPO_KEY_FPR" </dev/null || warn "could not remove the repository key from pacman"
        fi
    fi
    removed=true
}

do_uninstall() {
    removed=false
    if found=$(installed_package mumcord Mumcord "$AUR_PACKAGE"); then
        pkg=${found#* }
        need_root "Removing the package"
        case $found in
        deb\ *) $SUDO apt-get remove -y "$pkg" </dev/null ;;
        rpm\ *)
            if have dnf; then
                $SUDO dnf remove -y "$pkg" </dev/null
            elif have zypper; then
                $SUDO zypper --non-interactive remove "$pkg" </dev/null
            else
                $SUDO rpm -e "$pkg"
            fi
            ;;
        pacman\ *) $SUDO pacman -R --noconfirm "$pkg" </dev/null ;;
        pkg\ *) $SUDO pkg delete -y "$pkg" </dev/null ;;
        esac
        removed=true
    fi
    remove_repo_setup
    if [ -f "$APPIMAGE" ]; then
        rm -f "$APPIMAGE"
        removed=true
    fi
    # Only remove what Mumcord wrote; an entry the user edited stays.
    if [ -f "$ENTRY" ] && grep -qx "$MANAGED_KEY" "$ENTRY"; then
        rm -f "$ENTRY"
        refresh_menu
        removed=true
    fi
    for size in $ICON_SIZES; do
        rm -f "$DATA_HOME/icons/hicolor/$size/apps/$APP_ID.png"
    done
    icon=$(desktop_dir)/Mumcord.desktop
    if [ -f "$icon" ] && grep -qx "$WM_CLASS_LINE" "$icon"; then
        rm -f "$icon"
        removed=true
    fi
    if $removed; then
        printf '  %s%s%s Mumcord removed. Your settings and certificate were kept.\n' "$GREEN" "$MARK_OK" "$RESET"
    else
        printf '  %s%s%s Mumcord is not installed.\n' "$DIM" "$MARK_SKIP" "$RESET"
    fi
}

work=$(mktemp -d)
download_pid=
# Background jobs ignore Ctrl+C in a script, so a running download is stopped here.
cleanup() {
    if [ -n "$download_pid" ]; then
        kill "$download_pid" 2>/dev/null || true
        clear_line
        printf '%s[?25h' "$ESC"
    fi
    rm -rf "$work"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
if $uninstall; then
    do_uninstall
    exit 0
fi
do_install
