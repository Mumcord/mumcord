#!/bin/sh
# Exercise FreeBSD-only package metadata/output handling without installing anything. The
# native tools are faked, so this regression test can also run on a development workstation.
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fixture="$work/source"
mkdir -p "$work/bin" "$fixture/scripts" "$fixture/target/release" "$fixture/dist" \
  "$fixture/assets/backgrounds" "$fixture/packaging/freebsd" "$fixture/src-tauri/icons" \
  "$work/caller"
cp "$root/scripts/build-freebsd-pkg.sh" "$fixture/scripts/"
printf 'version = "9.8.7"\n' > "$fixture/Cargo.toml"
printf '#!/bin/sh\nexit 0\n' > "$fixture/target/release/mumcord"
chmod +x "$fixture/target/release/mumcord"
for file in LICENSE assets/backgrounds/test.webp packaging/freebsd/mumcord.desktop \
  packaging/freebsd/alsa-mumcord.conf packaging/freebsd/pkg-message \
  src-tauri/icons/32x32.png src-tauri/icons/128x128.png \
  src-tauri/icons/128x128@2x.png src-tauri/icons/icon.png; do
  printf 'fixture\n' > "$fixture/$file"
done
printf '#!/bin/sh\necho amd64\n' > "$work/bin/uname"
printf '#!/bin/sh\necho 15.1-RELEASE\n' > "$work/bin/freebsd-version"
printf '#!/bin/sh\necho 7\n' > "$work/bin/stat"
printf '#!/bin/sh\necho "libc.so.7 => /lib/libc.so.7 (0x0)"\n' > "$work/bin/ldd"
cat > "$work/bin/pkg" <<'SH'
#!/bin/sh
set -eu
case "$1" in
  query)
    name=${3#%n == }
    if [ "${MISSING_OSS:-0}" = 1 ] && [ "$name" = alsa-plugins ]; then exit 1; fi
    case "$4" in
      %n) printf '%s\n' "$name" ;;
      '%o %v') printf 'audio/%s 1.0\n' "$name" ;;
      *) echo "unexpected query: $*" >&2; exit 2 ;;
    esac
    ;;
  create)
    shift
    manifest="" output=""
    while [ "$#" -gt 0 ]; do
      case "$1" in
        -m) manifest=$2 ;;
        -o) output=$2 ;;
        -p|-r) ;;
        *) echo "unexpected create argument: $1" >&2; exit 2 ;;
      esac
      shift 2
    done
    mkdir -p "$output"
    cp "$manifest" "$output/mumcord-9.8.7.pkg"
    ;;
  *) echo "unexpected pkg invocation: $*" >&2; exit 2 ;;
esac
SH
chmod +x "$work/bin/"*
export PATH="$work/bin:$PATH"
cd "$work/caller"

check_package() {
  file=$1/mumcord-9.8.7.pkg
  [ -f "$file" ] || { echo "package disappeared from $1" >&2; exit 1; }
  for dep in alsa-plugins gstreamer1-libav gstreamer1-plugins-vpx gstreamer1-plugins-opus; do
    grep -q "^  $dep:" "$file" || { echo "missing runtime dependency: $dep" >&2; exit 1; }
  done
}

sh "$fixture/scripts/build-freebsd-pkg.sh" --no-build --no-strip "$work/absolute" > "$work/absolute.log" 2>&1
check_package "$work/absolute"
echo 'PASS absolute output and dynamically loaded runtime dependencies'
sh "$fixture/scripts/build-freebsd-pkg.sh" --no-build --no-strip 'relative output' > "$work/relative.log" 2>&1
check_package "$work/caller/relative output"
echo 'PASS relative output with spaces survives staging cleanup'
if MISSING_OSS=1 sh "$fixture/scripts/build-freebsd-pkg.sh" --no-build --no-strip "$work/missing" > "$work/missing.log" 2>&1; then
  echo 'package accepted a missing OSS backend' >&2
  exit 1
fi
grep -q 'missing build dependency alsa-plugins' "$work/missing.log"
echo 'PASS missing OSS plugin refuses packaging'
