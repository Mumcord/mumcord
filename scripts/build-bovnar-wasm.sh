#!/usr/bin/env bash
# Builds crates/plugin-sdk/bovnar-wasm/libbovnar_wasm.a: the Bovnar C library for the
# plugin target wasm32-unknown-unknown, so plugin authors need no C compiler. Next to it go
# the crate's LICENSE (MIT) and THIRD_PARTY_NOTICES.md (the unit tables compiled in).
#
# The sources are the published bovnar crate pinned in Cargo.lock (version and
# checksum), downloaded from crates.io and verified. The defines match the crate's
# build.rs for a release build with every unit profile on (spec FR-9.96).
#
# The compiler is the clang of a pinned wasi-sdk release (only its clang and llvm-ar are used,
# not its WASI libc), so the archive is byte-for-byte reproducible on any Linux x86_64 host.
# CI rebuilds it and fails when it differs from the committed one (ci.yml, job bovnar-wasm).
#
# Usage: scripts/build-bovnar-wasm.sh [output-dir]
#   default output-dir: crates/plugin-sdk/bovnar-wasm
#   WASI_SDK_PATH: an unpacked wasi-sdk-25.0 to use instead of downloading it.
# Needs: curl, sha256sum, tar.
set -euo pipefail

WASI_SDK_VERSION=25.0
WASI_SDK_SHA256=52640dde13599bf127a95499e61d6d640256119456d1af8897ab6725bcf3d89c

root="$(cd "$(dirname "$0")/.." && pwd)"
shim="$root/crates/plugin-sdk/bovnar-wasm"
outdir="$(realpath -m "${1:-$shim}")"
out="$outdir/libbovnar_wasm.a"

# The [[package]] block of bovnar in Cargo.lock: the one pinned version.
read -r version checksum < <(awk '
  /^\[\[package\]\]/ { name = ""; ver = ""; sum = "" }
  /^name = "bovnar"$/ { name = 1 }
  name && /^version = / { gsub(/"/, "", $3); ver = $3 }
  name && /^checksum = / { gsub(/"/, "", $3); print ver, $3; exit }
' "$root/Cargo.lock")
[ -n "${version:-}" ] && [ -n "${checksum:-}" ] || { echo "bovnar not found in Cargo.lock" >&2; exit 1; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

sdk="${WASI_SDK_PATH:-}"
if [ -z "$sdk" ]; then
  tarball="wasi-sdk-$WASI_SDK_VERSION-x86_64-linux.tar.gz"
  curl -sSfL -o "$work/$tarball" \
    "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${WASI_SDK_VERSION%%.*}/$tarball"
  echo "$WASI_SDK_SHA256  $work/$tarball" | sha256sum -c --quiet
  tar -xzf "$work/$tarball" -C "$work"
  sdk="$work/wasi-sdk-$WASI_SDK_VERSION-x86_64-linux"
fi
"$sdk/bin/clang" --version | grep -q "clang version 19.1.5-wasi-sdk" \
  || { echo "$sdk is not wasi-sdk $WASI_SDK_VERSION" >&2; exit 1; }
CLANG="$sdk/bin/clang"
AR="$sdk/bin/llvm-ar"
curl -sSfL -o "$work/bovnar.crate" "https://static.crates.io/crates/bovnar/bovnar-$version.crate"
echo "$checksum  $work/bovnar.crate" | sha256sum -c --quiet
tar -xzf "$work/bovnar.crate" -C "$work"
src="$work/bovnar-$version/c"

sources=(
  src/lexer/bovnar_lexer.c src/lexer/bovnar_state_table.c
  src/validator/bovnar_validator.c
  src/writer/bovnar_writer.c src/writer/bovnar_write_utils.c src/writer/bovnar_canon_observer.c
  src/io/bovnar_io.c
  src/dom/bovnar_dom.c src/dom/bovnar_dom_builder.c
  src/stream/bovnar_stream.c
  src/utils/bovnar_utils.c src/utils/bovnar_si_units.c src/utils/bovnar_currency.c
  src/utils/bovnar_profiles.c src/utils/bvn_int.c src/utils/bvn_float.c
  src/utils/bvn_gregorian_date.c src/utils/bvn_datetime.c
)
flags=(
  --target=wasm32-unknown-unknown -ffreestanding -nostdlibinc
  -isystem "$shim/include"
  -O2 -std=c99 -fno-strict-aliasing -fvisibility=hidden -DNDEBUG
  -DBVNR_WITH_UCUM_PROFILE=1 -DBVNR_WITH_UNECE_PROFILE=1 -DBVNR_WITH_QUDT_PROFILE=1
  -DBVNR_WITH_QUDT_QK_PROFILE=1 -DBVNR_WITH_UDUNITS_PROFILE=1 -DBVNR_WITH_OM_PROFILE=1
  -DBVNR_WITH_CF_PROFILE=1
  -I "$src/include" -I "$src/generated" -I "$src/src"
  # Keep build paths out of the objects so the archive is reproducible.
  "-ffile-prefix-map=$work=." "-ffile-prefix-map=$shim=."
)
for sub in dom io lexer stream utils validator writer; do flags+=(-I "$src/src/$sub"); done

objs=()
for f in "${sources[@]}" ../libc_extra.c; do
  in="$src/$f"; [ "$f" = ../libc_extra.c ] && in="$shim/libc_extra.c"
  o="$work/obj/$(basename "$f" .c).o"
  mkdir -p "$work/obj"
  "$CLANG" "${flags[@]}" -c "$in" -o "$o"
  objs+=("$(basename "$o")")
done
mkdir -p "$outdir"
cp "$work/bovnar-$version/LICENSE" "$work/bovnar-$version/THIRD_PARTY_NOTICES.md" "$outdir/"
rm -f "$out"
(cd "$work/obj" && "$AR" rcsD "$out" "${objs[@]}")
echo "built $out from bovnar $version ($(stat -c %s "$out") bytes)"
