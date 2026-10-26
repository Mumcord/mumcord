#!/bin/sh
# Installs Mumcord on Linux from the latest GitHub release:
#
#   curl -fsSL https://github.com/ToBee94/mumcord/releases/latest/download/install.sh | sh
#
# Debian/Ubuntu get the .deb (apt), Fedora/RHEL and openSUSE the .rpm (dnf/zypper), Arch the
# AUR package when an AUR helper is installed and the AUR has it; everything else gets the
# AppImage for the current user with an app-menu entry. Every download is checked against the
# release's SHA256SUMS before anything from it runs. sudo is used only to install or remove a
# package.
#
# Options (after `sh -s --` when piped):
#   --uninstall          remove Mumcord (settings and your certificate stay in place)
#   --desktop-icon       add a desktop icon without asking
#   --no-desktop-icon    do not add a desktop icon and do not ask
#   --yes                ask nothing and use the defaults (for scripts and CI)
# Environment: MUMCORD_DESKTOP_ICON=yes|no, MUMCORD_NONINTERACTIVE=1, and
# MUMCORD_RELEASE_URL to download from another release folder (tests use a local server).

set -eu

RELEASE_URL=${MUMCORD_RELEASE_URL:-https://github.com/ToBee94/mumcord/releases/latest/download}
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

say() { printf '%s\n' "$*"; }
warn() { printf 'Warning: %s\n' "$*" >&2; }
die() {
    printf 'Error: %s\n' "$*" >&2
    exit 1
}
have() { command -v "$1" >/dev/null 2>&1; }

usage() {
    sed -n '2,18s/^# \{0,1\}//p' "$0" 2>/dev/null || say "See https://github.com/ToBee94/mumcord#download"
}

uninstall=false
desktop_icon=${MUMCORD_DESKTOP_ICON:-ask}
noninteractive=${MUMCORD_NONINTERACTIVE:-}
for arg in "$@"; do
    case $arg in
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

# Questions go to the terminal, because with `curl | sh` standard input is the script itself.
can_ask() {
    [ -z "$noninteractive" ] && (: </dev/tty) 2>/dev/null
}

# ask_yes_no QUESTION: true on yes; the default (no) when nobody can answer.
ask_yes_no() {
    can_ask || return 1
    printf '%s [y/N] ' "$1" >/dev/tty
    read -r answer </dev/tty || return 1
    case $answer in
    [yY] | [yY][eE][sS]) return 0 ;;
    *) return 1 ;;
    esac
}

SUDO=
need_root() {
    if [ "$(id -u)" -eq 0 ]; then
        SUDO=
        return
    fi
    have sudo || die "$1 needs administrator rights, but sudo is not installed. Run this script as root instead."
    say "$1 needs administrator rights; sudo may ask for your password."
    SUDO=sudo
}

download() {
    if have curl; then
        curl -fsSL -o "$2" "$1"
    elif have wget; then
        wget -q -O "$2" "$1"
    else
        die "curl or wget is needed to download Mumcord"
    fi
}

sha256_of() {
    if have sha256sum; then
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
        [ -r /etc/os-release ] && . /etc/os-release && printf '%s %s' "${ID:-}" "${ID_LIKE:-}"
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
    esac
}

# fetch KIND: downloads the matching asset into $work, checks it and prints its path.
fetch() {
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
    [ -n "$matches" ] || die "the release has no $1 package for $(cpu)"
    [ "$(printf '%s\n' "$matches" | wc -l)" -eq 1 ] || die "the release lists more than one $1 package for $(cpu)"
    expected=${matches% *}
    name=${matches#* }
    say "Downloading $name ..." >&2
    download "$RELEASE_URL/$name" "$work/$name" || die "could not download $name"
    actual=$(sha256_of "$work/$name")
    if [ "$actual" != "$expected" ]; then
        rm -f "$work/$name"
        die "checksum mismatch for $name: the download is damaged or was tampered with. Nothing was installed."
    fi
    say "Checksum OK." >&2
    printf '%s\n' "$work/$name"
}

desktop_dir() {
    dir=
    have xdg-user-dir && dir=$(xdg-user-dir DESKTOP 2>/dev/null) || dir=
    printf '%s\n' "${dir:-$HOME/Desktop}"
}

# The package's or the user's menu entry, whichever exists.
menu_entry() {
    for file in "$ENTRY" /usr/share/applications/Mumcord.desktop /usr/share/applications/$APP_ID.desktop; do
        [ -f "$file" ] && grep -qx "$WM_CLASS_LINE" "$file" && printf '%s\n' "$file" && return
    done
    return 1
}

offer_desktop_icon() {
    case $desktop_icon in
    no) return ;;
    ask) ask_yes_no "Add a desktop icon?" || return 0 ;;
    esac
    source_entry=$(menu_entry) || {
        warn "no Mumcord menu entry found; skipping the desktop icon"
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
    say "Desktop icon added."
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
    file=$(fetch appimage)
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
    say "Mumcord installed to $APPIMAGE and added to the app menu."
    case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) say "Note: $BIN_DIR is not in your PATH; start Mumcord from the app menu." ;;
    esac
}

install_package() {
    case $1 in
    debian)
        file=$(fetch deb)
        # apt reads local packages as an unprivileged user, so the file must be readable.
        chmod 755 "$work"
        chmod 644 "$file"
        need_root "Installing the package"
        $SUDO apt-get install -y "$file" </dev/null
        ;;
    fedora)
        file=$(fetch rpm)
        need_root "Installing the package"
        $SUDO dnf install -y "$file" </dev/null
        ;;
    suse)
        file=$(fetch rpm)
        need_root "Installing the package"
        # The release's .rpm files are not GPG-signed; the SHA256SUMS check above vouches for them.
        $SUDO zypper --non-interactive install --allow-unsigned-rpm "$file" </dev/null
        ;;
    esac
    say "Mumcord installed."
}

# The AppImage has no H.264 decoder (patent-encumbered); the system's gst-libav adds it
# (FR-2.41a). The distribution is unknown here, so the package manager decides the command.
codec_hint() {
    if have apt-get; then
        cmd="sudo apt install gstreamer1.0-libav"
    elif have dnf; then
        cmd="sudo dnf install gstreamer1-libav (gst-libav comes from RPM Fusion; enable it first)"
    elif have pacman; then
        cmd="sudo pacman -S gst-libav"
    elif have zypper; then
        cmd="sudo zypper install gstreamer-plugins-libav"
    else
        say "Some videos need H.264 support. Install gst-libav (the GStreamer libav plugin) with your system's package manager."
        return
    fi
    say "Some videos need H.264 support. Install gst-libav: $cmd"
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
    family=$(distro_family)
    cpu >/dev/null
    case $family in
    debian | fedora | suse) install_package "$family" ;;
    arch)
        if helper=$(aur_helper) && aur_has_package "$helper"; then
            say "Installing $AUR_PACKAGE from the AUR with $helper."
            if can_ask; then
                "$helper" -S --needed "$AUR_PACKAGE" </dev/tty
            else
                "$helper" -S --needed --noconfirm "$AUR_PACKAGE" </dev/null
            fi
        else
            install_appimage
            say "Tip: if the AUR has $AUR_PACKAGE, installing it with an AUR helper gets Mumcord updates with your system."
        fi
        ;;
    *) install_appimage ;;
    esac
    offer_desktop_icon
    # Last, so it is not lost above the desktop icon question.
    if [ "$family" = other ]; then
        codec_hint
    fi
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
    done
    return 1
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
        esac
        removed=true
    fi
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
        say "Mumcord removed. Your settings and certificate were kept."
    else
        say "Mumcord is not installed."
    fi
}

if $uninstall; then
    do_uninstall
    exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
trap 'exit 1' HUP INT TERM
do_install
