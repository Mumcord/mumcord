#!/bin/sh
# Builds a fake Mumcord release folder for testing scripts/install.sh: the six Linux assets
# with the real release file names (tiny packages that install a stub `mumcord` and the menu
# entry), SHA256SUMS and install.sh in OUT_DIR/good, and the same release with every asset
# altered after the checksums were written in OUT_DIR/bad. Real packages are checked in build-and-smoke.yml; these
# keep the installer test fast and independent of the full app build.
#
# Usage: make-fake-release.sh OUT_DIR   (needs dpkg-deb and rpmbuild)

set -eu

out=$1/good
bad=$1/bad
version=0.0.1
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

mkdir -p "$out"

# Package payload: the files a real package installs that the installer and tests look at.
root=$work/root
mkdir -p "$root/usr/bin" "$root/usr/share/applications" "$root/usr/share/icons/hicolor/128x128/apps"
printf '#!/bin/sh\necho "fake Mumcord %s"\n' "$version" >"$root/usr/bin/mumcord"
chmod 755 "$root/usr/bin/mumcord"
cat >"$root/usr/share/applications/Mumcord.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Mumcord
Exec=mumcord %u
Icon=mumcord
Terminal=false
Categories=Network;Chat;AudioVideo;
MimeType=x-scheme-handler/mumble;
StartupWMClass=app.mumcord.client
EOF
printf 'fake png' >"$root/usr/share/icons/hicolor/128x128/apps/mumcord.png"

for arch in amd64 arm64; do
    pkg=$work/deb-$arch
    mkdir -p "$pkg/DEBIAN"
    cp -R "$root/usr" "$pkg/"
    cat >"$pkg/DEBIAN/control" <<EOF
Package: mumcord
Version: $version
Architecture: $arch
Maintainer: Mumcord test <test@invalid>
Description: Fake Mumcord package for installer tests
EOF
    dpkg-deb --root-owner-group --build "$pkg" "$out/Mumcord_${version}_${arch}.deb" >/dev/null
done

cat >"$work/mumcord.spec" <<EOF
Name: Mumcord
Version: $version
Release: 1
Summary: Fake Mumcord package for installer tests
License: AGPL-3.0-or-later
AutoReqProv: no

%description
Fake Mumcord package for installer tests.

%install
mkdir -p %{buildroot}
cp -R $root/usr %{buildroot}/

%files
/usr/bin/mumcord
/usr/share/applications/Mumcord.desktop
/usr/share/icons/hicolor/128x128/apps/mumcord.png
EOF
for arch in x86_64 aarch64; do
    rpmbuild --quiet -bb --target "$arch" --define "_topdir $work/rpm" "$work/mumcord.spec" >/dev/null
    cp "$work/rpm/RPMS/$arch/Mumcord-$version-1.$arch.rpm" "$out/"
done

# A stub AppImage: answers --appimage-extract like the real runtime does for the icon.
for arch in amd64 aarch64; do
    file=$out/Mumcord_${version}_${arch}.AppImage
    cat >"$file" <<'EOF'
#!/bin/sh
if [ "${1:-}" = --appimage-extract ]; then
    mkdir -p squashfs-root
    printf 'fake png' >squashfs-root/Mumcord.png
    exit 0
fi
echo "fake Mumcord AppImage"
EOF
    chmod 755 "$file"
done

cp "$here/../install.sh" "$out/install.sh"
(cd "$out" && sha256sum -- Mumcord_* Mumcord-* install.sh >SHA256SUMS)

rm -rf "$bad"
cp -R "$out" "$bad"
for file in "$bad"/Mumcord_* "$bad"/Mumcord-*; do
    printf 'tampered' >>"$file"
done
