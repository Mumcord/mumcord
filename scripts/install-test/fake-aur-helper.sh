#!/bin/sh
# Installs a stub `paru` into the container that knows no AUR packages, as long as mumcord-bin
# is not in the AUR. Any install attempt fails, so the test catches an installer that tries it.

set -eu

cat >/usr/local/bin/paru <<'EOF'
#!/bin/sh
case ${1:-} in
-Si)
    echo "error: package '$2' was not found" >&2
    exit 1
    ;;
esac
echo "fake paru: unexpected call: $*" >&2
exit 1
EOF
chmod 755 /usr/local/bin/paru
