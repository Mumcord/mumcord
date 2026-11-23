#!/usr/bin/env bash
# Exercise the native build entry points without downloading or compiling their dependencies.
# Explicit limits must reach the build tools; without them the existing defaults must stay.
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"
export BUILD_LOG="$work/log" TEST_OS=Linux
cat > "$work/bin/stub" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
name=${0##*/}
printf '%s' "$name" >> "$BUILD_LOG"
printf ' <%s>' "$@" >> "$BUILD_LOG"
printf '\n' >> "$BUILD_LOG"
install_tree() {
  mkdir -p "$1/include/"{vpx,svt-av1,dav1d,vpl} "$1/lib/pkgconfig"
  touch "$1/include/vpx/vpx_encoder.h" "$1/include/svt-av1/EbSvtAv1Enc.h" "$1/include/dav1d/dav1d.h"
  touch "$1/lib/"{libvpx.a,libSvtAv1Enc.a,libdav1d.a,libvpl.a}
  printf 'prefix=/unused\n' | tee "$1/lib/pkgconfig/"{SvtAv1Enc,dav1d,vpl}.pc >/dev/null
}
case "$name" in
  uname) echo "$TEST_OS" ;;
  nproc) echo 7 ;;
  sysctl) echo 6 ;;
  curl | lipo) ;;
  tar)
    dest=${@: -1}
    mkdir -p "$dest/"{libvpx-1.15.2,SVT-AV1-v4.2.0,dav1d-1.5.5,libvpl-2.17.0,nv-codec-headers-n13.0.19.0/include/ffnvcodec,amf-headers-v1.5.3/AMF,libva-2.22.0/va/drm}
    va="$dest/libva-2.22.0/va"
    touch "$va/"{va.h,va_backend.h,va_backend_prot.h,va_backend_vpp.h,va_compat.h,va_dec_test.h,va_drmcommon.h,va_enc_test.h,va_fei_test.h,va_str.h,va_tpi.h,va_prot.h,va_vpp.h,drm/va_drm.h,va_version.h.in}
    printf 'va_api_major_version = 1\nva_api_minor_version = 22\nva_api_micro_version = 0\n' > "$va/../meson.build"
    cat > "$dest/libvpx-1.15.2/configure" <<'CONFIGURE'
#!/usr/bin/env bash
for arg; do
  case "$arg" in --prefix=*) printf '%s' "${arg#*=}" > prefix ;; esac
done
CONFIGURE
    chmod +x "$dest/libvpx-1.15.2/configure"
    ;;
  make) if [[ ${1:-} == install ]]; then install_tree "$(<prefix)"; fi ;;
  cmake)
    if [[ $1 == -S ]]; then
      shift 2
      [[ $1 == -B ]]; dir=$2
      mkdir -p "$dir"
      for arg; do
        case "$arg" in -DCMAKE_INSTALL_PREFIX=*) printf '%s' "${arg#*=}" > "$dir/prefix" ;; esac
      done
    elif [[ $1 == --install ]]; then install_tree "$(<"$2/prefix")"
    fi
    ;;
  meson)
    if [[ $1 == setup ]]; then
      dir=$2; mkdir -p "$dir"
      for arg; do
        case "$arg" in --prefix=*) printf '%s' "${arg#*=}" > "$dir/prefix" ;; esac
      done
    elif [[ $1 == install ]]; then install_tree "$(<"$3/prefix")"
    fi
    ;;
esac
EOF
chmod +x "$work/bin/stub"
for command in uname nproc sysctl curl tar make cmake meson lipo; do
  ln -s stub "$work/bin/$command"
done
export PATH="$work/bin:$PATH"
for TEST_OS in Linux Darwin; do
  export TEST_OS
  for script in libvpx svt-av1 dav1d hwenc-sdk; do
    [[ $TEST_OS == Darwin && $script == hwenc-sdk ]] && continue
    for limit in default 1; do
      : > "$BUILD_LOG"
      unset CARGO_BUILD_JOBS CMAKE_BUILD_PARALLEL_LEVEL
      if [[ $limit != default ]]; then
        export CARGO_BUILD_JOBS=$limit CMAKE_BUILD_PARALLEL_LEVEL=$limit
      fi
      bash "$repo/scripts/build-$script.sh" "$work/$TEST_OS-$script-$limit" > "$work/output" 2>&1 || {
        cat "$work/output" >&2; exit 1;
      }
      case "$script" in
        libvpx)
          jobs=$limit
          if [[ $limit == default ]]; then
            jobs=7; [[ $TEST_OS == Darwin ]] && jobs=6
          fi
          expected="make <-j$jobs>"
          ;;
        dav1d)
          expected='meson <compile> .*'
          if [[ $limit != default ]]; then expected+=" <-j> <$limit>"; fi
          ;;
        *)
          expected='cmake <--build> .* <--config> <Release> <--parallel>'
          if [[ $limit != default ]]; then expected+=" <$limit>"; fi
          ;;
      esac
      grep -Eq "^$expected$" "$BUILD_LOG" || {
        echo "FAIL: $TEST_OS $script ($limit): expected $expected" >&2
        cat "$BUILD_LOG" >&2; exit 1;
      }
    done
  done
done
echo 'Native build job limits forwarded; Linux and macOS defaults unchanged.'
